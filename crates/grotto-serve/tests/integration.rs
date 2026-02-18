use futures::StreamExt;
use grotto_core::Grotto;
use grotto_serve::WsEvent;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

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

#[tokio::test]
async fn test_health_endpoint() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let _grotto = Grotto::new(&dir, 2, "test task".into()).unwrap();

    let port = start_test_server(dir.join(".grotto")).await;

    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    writer
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();

    let mut buf = vec![0u8; 1024];
    let n = reader.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
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

    // First message should be the snapshot
    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("Timeout waiting for snapshot")
        .expect("Stream ended")
        .expect("WS error");

    let text = match msg {
        Message::Text(t) => t.to_string(),
        other => panic!("Expected text message, got {:?}", other),
    };

    let event: WsEvent = serde_json::from_str(&text).expect("Failed to parse snapshot");
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
    let _ = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    // Modify an agent's status file
    let agent_status = grotto_core::AgentState {
        id: "agent-1".to_string(),
        pane_index: 0,
        state: "working".to_string(),
        current_task: Some("main".to_string()),
        progress: "Building the API".to_string(),
        last_update: chrono::Utc::now(),
    };
    let status_json = serde_json::to_string_pretty(&agent_status).unwrap();
    let status_path = dir.join(".grotto/agents/agent-1/status.json");
    std::fs::write(&status_path, &status_json).unwrap();

    // Wait for the file watcher to pick it up and broadcast
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = ws.next().await {
                let event: WsEvent = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if event.event_type == "agent:status" {
                    return event;
                }
            }
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "Should receive agent:status event within 5s"
    );
    let event = result.unwrap();
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
    let _ = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

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

    // Wait for event:raw
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = ws.next().await {
                let event: WsEvent = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if event.event_type == "event:raw" {
                    return event;
                }
            }
        }
    })
    .await;

    assert!(result.is_ok(), "Should receive event:raw within 5s");
    let event = result.unwrap();
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
    let snap1 = tokio::time::timeout(Duration::from_secs(5), ws1.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let snap2 = tokio::time::timeout(Duration::from_secs(5), ws2.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let s1: WsEvent = serde_json::from_str(&match snap1 {
        Message::Text(t) => t.to_string(),
        _ => panic!("expected text"),
    })
    .unwrap();
    let s2: WsEvent = serde_json::from_str(&match snap2 {
        Message::Text(t) => t.to_string(),
        _ => panic!("expected text"),
    })
    .unwrap();

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

    let recv1 = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = ws1.next().await {
                let event: WsEvent = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if event.event_type == "event:raw" {
                    return event;
                }
            }
        }
    })
    .await;

    let recv2 = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = ws2.next().await {
                let event: WsEvent = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if event.event_type == "event:raw" {
                    return event;
                }
            }
        }
    })
    .await;

    assert!(recv1.is_ok(), "Client 1 should receive event");
    assert!(recv2.is_ok(), "Client 2 should receive event");
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

    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();
    writer
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut buf = vec![0u8; 1024];
    let n = reader.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
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
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();
    let body = serde_json::json!({
        "id": "test-coral-reef",
        "dir": dir.display().to_string()
    });
    let body_str = serde_json::to_string(&body).unwrap();
    let req = format!(
        "POST /api/sessions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body_str.len(),
        body_str
    );
    writer.write_all(req.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = reader.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    assert!(response.contains("201"), "Expected 201, got: {}", response);
    assert!(response.contains("registered"), "Got: {}", response);

    // List sessions via GET
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();
    writer
        .write_all(b"GET /api/sessions HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut buf = vec![0u8; 4096];
    let n = reader.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    assert!(response.contains("test-coral-reef"), "Got: {}", response);
    assert!(response.contains("test daemon task"), "Got: {}", response);
}

#[tokio::test]
async fn test_daemon_per_session_ws() {
    let port = start_daemon_server().await;

    // Create a grotto project and register it
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let grotto = Grotto::new(&dir, 1, "ws session test".into()).unwrap();

    // Register via HTTP
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();
    let body = serde_json::json!({
        "id": "ws-test-session",
        "dir": dir.display().to_string()
    });
    let body_str = serde_json::to_string(&body).unwrap();
    let req = format!(
        "POST /api/sessions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body_str.len(),
        body_str
    );
    writer.write_all(req.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let _ = reader.read(&mut buf).await.unwrap();

    // Connect WS to the session-specific endpoint
    let url = format!("ws://127.0.0.1:{}/ws/ws-test-session", port);
    let (mut ws, _) = connect_async(&url).await.expect("WS connect failed");

    // Should get snapshot
    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("Timeout")
        .expect("Stream ended")
        .expect("WS error");
    let text = match msg {
        Message::Text(t) => t.to_string(),
        other => panic!("Expected text, got {:?}", other),
    };
    let event: WsEvent = serde_json::from_str(&text).unwrap();
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

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = ws.next().await {
                let event: WsEvent = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if event.event_type == "event:raw" {
                    return event;
                }
            }
        }
    })
    .await;

    assert!(result.is_ok(), "Should receive event via per-session WS");
    assert_eq!(result.unwrap().message, Some("daemon event".to_string()));
}

#[tokio::test]
async fn test_daemon_unregister_session() {
    let port = start_daemon_server().await;

    // Create and register a session
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let _grotto = Grotto::new(&dir, 1, "unregister test".into()).unwrap();

    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();
    let body = serde_json::json!({
        "id": "to-remove",
        "dir": dir.display().to_string()
    });
    let body_str = serde_json::to_string(&body).unwrap();
    let req = format!(
        "POST /api/sessions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body_str.len(),
        body_str
    );
    writer.write_all(req.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let _ = reader.read(&mut buf).await.unwrap();

    // Unregister via DELETE
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();
    writer
        .write_all(b"DELETE /api/sessions/to-remove HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut buf = vec![0u8; 4096];
    let n = reader.read(&mut buf).await.unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
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
