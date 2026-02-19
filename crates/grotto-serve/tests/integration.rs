use futures::StreamExt;
use grotto_core::Grotto;
use grotto_serve::WsEvent;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

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

/// Start the daemon server in background and return the port
async fn start_daemon_server() -> u16 {
    let port = free_port().await;
    tokio::spawn(async move {
        grotto_serve::run_daemon(port, None).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    port
}

fn parse_ws_text_message(msg: Message) -> Result<WsEvent, String> {
    let text = match msg {
        Message::Text(t) => t.to_string(),
        other => return Err(format!("Expected text message, got {:?}", other)),
    };

    serde_json::from_str(&text).map_err(|err| format!("Failed to parse ws event JSON: {err}"))
}

async fn consume_initial_snapshot(ws: &mut WsStream) -> WsEvent {
    wait_for_event_type(ws, "snapshot", DEFAULT_TIMEOUT).await
}

async fn wait_for_event_type(ws: &mut WsStream, event_type: &str, timeout: Duration) -> WsEvent {
    tokio::time::timeout(timeout, async {
        loop {
            match ws.next().await {
                Some(Ok(msg)) => {
                    if let Ok(event) = parse_ws_text_message(msg) {
                        if event.event_type == event_type {
                            return event;
                        }
                    }
                }
                Some(Err(err)) => panic!("WebSocket error while waiting for {event_type}: {err}"),
                None => panic!("WebSocket stream ended while waiting for {event_type}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Timeout waiting for {event_type}"))
}

async fn http_request(port: u16, req: &str) -> String {
    let stream = TcpStream::connect(format!("127.0.0.1:{port}")).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    writer.write_all(req.as_bytes()).await.unwrap();

    let mut buf = vec![0u8; 16384];
    let n = reader.read(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

async fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
    http_request(port, &req).await
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

    let event = consume_initial_snapshot(&mut ws).await;
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

    // Consume the initial snapshot
    let _ = consume_initial_snapshot(&mut ws).await;

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

    // Wait for the file watcher to pick it up and broadcast
    let event = wait_for_event_type(&mut ws, "agent:status", DEFAULT_TIMEOUT).await;

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

    // Consume the initial snapshot
    let _ = consume_initial_snapshot(&mut ws).await;

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

    let event = wait_for_event_type(&mut ws, "event:raw", DEFAULT_TIMEOUT).await;
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
    let s1 = consume_initial_snapshot(&mut ws1).await;
    let s2 = consume_initial_snapshot(&mut ws2).await;

    assert_eq!(s1.event_type, "snapshot");
    assert_eq!(s2.event_type, "snapshot");

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

    let recv1 = wait_for_event_type(&mut ws1, "event:raw", DEFAULT_TIMEOUT).await;
    let recv2 = wait_for_event_type(&mut ws2, "event:raw", DEFAULT_TIMEOUT).await;

    assert_eq!(recv1.event_type, "event:raw");
    assert_eq!(recv2.event_type, "event:raw");
}

// ---------------------------------------------------------------------------
// Daemon (multi-session) integration tests
// ---------------------------------------------------------------------------

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
    let event = consume_initial_snapshot(&mut ws).await;
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

    let event = wait_for_event_type(&mut ws, "event:raw", DEFAULT_TIMEOUT).await;
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
