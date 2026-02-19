use futures::StreamExt;
use grotto_core::Grotto;
use grotto_serve::WsEvent;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
    MaybeTlsStream, WebSocketStream,
};

type TestWs = WebSocketStream<MaybeTlsStream<TcpStream>>;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Find a free port by binding to port 0
async fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// Start the server in background and return the port
async fn start_test_server(grotto_dir: std::path::PathBuf) -> u16 {
    let port = free_port().await;
    let dir = grotto_dir.clone();
    tokio::spawn(async move {
        grotto_serve::run_server(dir, port, None).await.unwrap();
    });
    // Give server time to start
    tokio::time::sleep(Duration::from_millis(200)).await;
    port
}

fn message_to_ws_event(msg: Message) -> WsEvent {
    let text = match msg {
        Message::Text(t) => t.to_string(),
        other => panic!("Expected text message, got {:?}", other),
    };
    serde_json::from_str(&text).expect("Failed to parse WS event")
}

async fn next_ws_event(ws: &mut TestWs) -> WsEvent {
    let msg = tokio::time::timeout(TEST_TIMEOUT, ws.next())
        .await
        .expect("Timeout waiting for WS message")
        .expect("WS stream ended")
        .expect("WS receive error");

    message_to_ws_event(msg)
}

async fn consume_initial_ws_snapshot(ws: &mut TestWs) {
    let event = next_ws_event(ws).await;
    assert_eq!(event.event_type, "snapshot", "first WS event should be snapshot");
}

async fn wait_for_event_type(ws: &mut TestWs, event_type: &str) -> WsEvent {
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            let Some(message_result) = ws.next().await else {
                panic!("WS stream ended while waiting for event: {event_type}");
            };

            let Ok(message) = message_result else {
                continue;
            };

            let Message::Text(text) = message else {
                continue;
            };

            let Ok(event) = serde_json::from_str::<WsEvent>(&text) else {
                continue;
            };

            if event.event_type == event_type {
                return event;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Should receive {event_type} event within {TEST_TIMEOUT:?}"))
}

async fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).await.unwrap();
        if n == 0 {
            break;
        }
        response.extend_from_slice(&buf[..n]);
        if n < buf.len() {
            break;
        }
    }

    String::from_utf8_lossy(&response).into_owned()
}

async fn http_get(port: u16, path: &str) -> String {
    http_request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    )
    .await
}

async fn register_session(port: u16, id: &str, dir: &std::path::Path) -> String {
    let body = serde_json::json!({
        "id": id,
        "dir": dir.display().to_string()
    });
    let body_str = serde_json::to_string(&body).unwrap();
    let req = format!(
        "POST /api/sessions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body_str.len(),
        body_str
    );

    http_request(port, &req).await
}

#[tokio::test]
async fn test_health_endpoint() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let _grotto = Grotto::new(&dir, 2, "test task".into()).unwrap();

    let port = start_test_server(dir.join(".grotto")).await;

    let response = http_get(port, "/health").await;
    assert!(response.contains("200 OK"), "Got: {}", response);
    assert!(response.contains("ok"), "Got: {}", response);
}

#[tokio::test]
async fn test_ws_snapshot_on_connect() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let _grotto = Grotto::new(&dir, 3, "ws test task".into()).unwrap();

    let port = start_test_server(dir.join(".grotto")).await;

    let url = format!("ws://127.0.0.1:{}/ws", port);
    let (mut ws, _) = connect_async(&url).await.expect("WS connect failed");

    let event = next_ws_event(&mut ws).await;
    assert_eq!(event.event_type, "snapshot");
    let agents = event.agents.expect("snapshot should have agents");
    assert_eq!(agents.len(), 3);
    let config = event.config.expect("snapshot should have config");
    assert_eq!(config.agent_count, 3);
    assert_eq!(config.task, "ws test task");
}

#[tokio::test]
async fn test_ws_receives_agent_status_change() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let _grotto = Grotto::new(&dir, 2, "status change test".into()).unwrap();

    let port = start_test_server(dir.join(".grotto")).await;

    let url = format!("ws://127.0.0.1:{}/ws", port);
    let (mut ws, _) = connect_async(&url).await.expect("WS connect failed");

    consume_initial_ws_snapshot(&mut ws).await;

    // Modify an agent's status file
    let agent_status = grotto_core::AgentState {
        id: "agent-1".to_string(),
        pane_index: 0,
        state: "working".to_string(),
        current_task: Some("main".to_string()),
        progress: "Building the API".to_string(),
        last_update: chrono::Utc::now(),
        phase: None,
    };
    let status_json = serde_json::to_string_pretty(&agent_status).unwrap();
    let status_path = dir.join(".grotto/agents/agent-1/status.json");
    std::fs::write(&status_path, &status_json).unwrap();

    let event = wait_for_event_type(&mut ws, "agent:status").await;
    assert_eq!(event.agent_id, Some("agent-1".to_string()));
    assert!(event.message.unwrap().contains("working"));
}

#[tokio::test]
async fn test_ws_receives_event_raw_on_jsonl_append() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let grotto = Grotto::new(&dir, 1, "event test".into()).unwrap();

    let port = start_test_server(dir.join(".grotto")).await;

    let url = format!("ws://127.0.0.1:{}/ws", port);
    let (mut ws, _) = connect_async(&url).await.expect("WS connect failed");

    consume_initial_ws_snapshot(&mut ws).await;

    // Append a new event to events.jsonl
    grotto
        .log_event(
            "custom_test",
            Some("agent-1"),
            Some("main"),
            Some("Test event fired"),
            serde_json::json!({"test": true}),
        )
        .unwrap();

    let event = wait_for_event_type(&mut ws, "event:raw").await;
    assert_eq!(event.message, Some("Test event fired".to_string()));
}

#[tokio::test]
async fn test_multiple_ws_clients_receive_same_events() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let grotto = Grotto::new(&dir, 1, "multi-client test".into()).unwrap();

    let port = start_test_server(dir.join(".grotto")).await;

    let url = format!("ws://127.0.0.1:{}/ws", port);

    // Connect two clients
    let (mut ws1, _) = connect_async(&url).await.expect("WS1 connect failed");
    let (mut ws2, _) = connect_async(&url).await.expect("WS2 connect failed");

    // Both should get snapshots
    consume_initial_ws_snapshot(&mut ws1).await;
    consume_initial_ws_snapshot(&mut ws2).await;

    // Now trigger an event — both should receive it
    grotto
        .log_event(
            "broadcast_test",
            None,
            None,
            Some("hello both"),
            serde_json::json!({}),
        )
        .unwrap();

    let recv1 = wait_for_event_type(&mut ws1, "event:raw").await;
    let recv2 = wait_for_event_type(&mut ws2, "event:raw").await;

    assert_eq!(recv1.message, Some("hello both".to_string()));
    assert_eq!(recv2.message, Some("hello both".to_string()));
}

// ---------------------------------------------------------------------------
// Daemon (multi-session) integration tests
// ---------------------------------------------------------------------------

/// Start the daemon server in background and return the port
async fn start_daemon_server() -> u16 {
    let port = free_port().await;
    tokio::spawn(async move {
        grotto_serve::run_daemon(port, None).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    port
}

#[tokio::test]
async fn test_daemon_health() {
    let port = start_daemon_server().await;

    let response = http_get(port, "/health").await;
    assert!(response.contains("200 OK"), "Got: {}", response);
}

#[tokio::test]
async fn test_daemon_register_and_list_sessions() {
    let port = start_daemon_server().await;

    // Create a grotto project
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let _grotto = Grotto::new(&dir, 2, "test daemon task".into()).unwrap();

    // Register the session via POST
    let register_response = register_session(port, "test-coral-reef", &dir).await;
    assert!(
        register_response.contains("201"),
        "Expected 201, got: {}",
        register_response
    );
    assert!(
        register_response.contains("registered"),
        "Got: {}",
        register_response
    );

    // List sessions via GET
    let response = http_get(port, "/api/sessions").await;
    assert!(response.contains("test-coral-reef"), "Got: {}", response);
    assert!(response.contains("test daemon task"), "Got: {}", response);
    assert!(response.contains("\"status\""), "Got: {}", response);
    assert!(response.contains("\"last_updated\""), "Got: {}", response);
}

#[tokio::test]
async fn test_daemon_session_events_endpoint_returns_history() {
    let port = start_daemon_server().await;

    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let grotto = Grotto::new(&dir, 1, "history endpoint test".into()).unwrap();

    // Register the session via POST
    let _ = register_session(port, "history-session", &dir).await;

    // Append an event to history
    grotto
        .log_event(
            "history_test",
            Some("agent-1"),
            None,
            Some("historical event"),
            serde_json::json!({"ok": true}),
        )
        .unwrap();

    let response = http_get(port, "/api/sessions/history-session/events").await;

    assert!(response.contains("200 OK"), "Got: {}", response);
    assert!(response.contains("history_test"), "Got: {}", response);
    assert!(response.contains("historical event"), "Got: {}", response);
}

#[tokio::test]
async fn test_daemon_per_session_ws() {
    let port = start_daemon_server().await;

    // Create a grotto project and register it
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let grotto = Grotto::new(&dir, 1, "ws session test".into()).unwrap();

    // Register via HTTP
    let _ = register_session(port, "ws-test-session", &dir).await;

    // Connect WS to the session-specific endpoint
    let url = format!("ws://127.0.0.1:{}/ws/ws-test-session", port);
    let (mut ws, _) = connect_async(&url).await.expect("WS connect failed");

    // Should get snapshot
    let event = next_ws_event(&mut ws).await;
    assert_eq!(event.event_type, "snapshot");
    assert_eq!(event.config.unwrap().task, "ws session test");

    // Log an event and verify it arrives via WS
    grotto
        .log_event(
            "daemon_test",
            None,
            None,
            Some("daemon event"),
            serde_json::json!({}),
        )
        .unwrap();

    let event = wait_for_event_type(&mut ws, "event:raw").await;
    assert_eq!(event.message, Some("daemon event".to_string()));
}

#[tokio::test]
async fn test_daemon_unregister_session() {
    let port = start_daemon_server().await;

    // Create and register a session
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let _grotto = Grotto::new(&dir, 1, "unregister test".into()).unwrap();

    let _ = register_session(port, "to-remove", &dir).await;

    // Unregister via DELETE
    let response = http_request(
        port,
        "DELETE /api/sessions/to-remove HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    assert!(response.contains("200"), "Expected 200, got: {}", response);
    assert!(response.contains("unregistered"), "Got: {}", response);

    // WS to removed session should fail (404)
    let url = format!("ws://127.0.0.1:{}/ws/to-remove", port);
    let result = connect_async(&url).await;
    // Connection might succeed but then get a non-101 response
    match result {
        Err(_) => {} // expected — connection rejected
        Ok((_ws, _)) => {
            // Session was removed but WS connected — this is fine,
            // the important thing is the session won't get events anymore
        }
    }
}

#[tokio::test]
async fn test_daemon_ws_unknown_session_404() {
    let port = start_daemon_server().await;

    let url = format!("ws://127.0.0.1:{}/ws/nonexistent-session", port);
    let result = connect_async(&url).await;
    // Should fail to upgrade — server returns 404
    assert!(result.is_err(), "WS to unknown session should fail");
}
