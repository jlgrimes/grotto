use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
    routing::get,
};
use futures::{SinkExt, StreamExt};
use grotto_core::{AgentState, Event, Grotto};
use notify::{RecommendedWatcher, RecursiveMode, Watcher, EventKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    // snapshot-only fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<HashMap<String, AgentState>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<Vec<TaskInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<ConfigInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub description: String,
    pub status: String,
    pub claimed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigInfo {
    pub agent_count: usize,
    pub task: String,
    pub project_dir: String,
}

#[derive(Clone)]
pub struct AppState {
    pub tx: broadcast::Sender<String>,
    pub grotto_dir: PathBuf,
}

impl AppState {
    fn build_snapshot(&self) -> WsEvent {
        let grotto = Grotto::load(
            self.grotto_dir.parent().unwrap_or(Path::new(".")),
        );

        match grotto {
            Ok(g) => {
                let tasks = parse_task_board(&self.grotto_dir.join("tasks.md"));
                WsEvent {
                    event_type: "snapshot".to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    agent_id: None,
                    task_id: None,
                    message: Some("Full state snapshot".to_string()),
                    data: None,
                    agents: Some(g.agents),
                    tasks: Some(tasks),
                    config: Some(ConfigInfo {
                        agent_count: g.config.agent_count,
                        task: g.config.task.clone(),
                        project_dir: g.config.project_dir.display().to_string(),
                    }),
                }
            }
            Err(_) => WsEvent {
                event_type: "snapshot".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                agent_id: None,
                task_id: None,
                message: Some("No grotto state found".to_string()),
                data: None,
                agents: Some(HashMap::new()),
                tasks: Some(Vec::new()),
                config: None,
            },
        }
    }
}

fn parse_task_board(path: &Path) -> Vec<TaskInfo> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut tasks = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let mut task = None;

        if let Some(rest) = line.strip_prefix("⭕ ") {
            task = parse_task_line(rest, "open");
        } else if let Some(rest) = line.strip_prefix("🟡 ") {
            task = parse_task_line(rest, "claimed");
        } else if let Some(rest) = line.strip_prefix("🔄 ") {
            task = parse_task_line(rest, "in_progress");
        } else if let Some(rest) = line.strip_prefix("✅ ") {
            task = parse_task_line(rest, "completed");
        } else if let Some(rest) = line.strip_prefix("🚫 ") {
            task = parse_task_line(rest, "blocked");
        }

        if let Some(mut t) = task {
            // Check next line for "Claimed by:" info
            if i + 1 < lines.len() {
                let next = lines[i + 1].trim();
                if let Some(agent) = next.strip_prefix("- Claimed by: ") {
                    t.claimed_by = Some(agent.to_string());
                }
            }
            tasks.push(t);
        }

        i += 1;
    }
    tasks
}

fn parse_task_line(rest: &str, status: &str) -> Option<TaskInfo> {
    // Format: **task-id** - description
    let rest = rest.strip_prefix("**")?;
    let (id, remainder) = rest.split_once("**")?;
    let description = remainder.strip_prefix(" - ").unwrap_or(remainder).trim();
    Some(TaskInfo {
        id: id.to_string(),
        description: description.to_string(),
        status: status.to_string(),
        claimed_by: None,
    })
}

pub async fn run_server(grotto_dir: PathBuf, port: u16, web_dir: Option<PathBuf>) -> std::io::Result<()> {
    let (tx, _rx) = broadcast::channel::<String>(256);
    let state = AppState {
        tx: tx.clone(),
        grotto_dir: grotto_dir.clone(),
    };

    // Start file watcher in background
    let watcher_tx = tx.clone();
    let watcher_dir = grotto_dir.clone();
    tokio::spawn(async move {
        if let Err(e) = run_file_watcher(watcher_dir, watcher_tx).await {
            eprintln!("File watcher error: {}", e);
        }
    });

    let mut app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_handler))
        .with_state(state);

    // Serve static web files if directory exists
    if let Some(web_path) = web_dir {
        if web_path.exists() {
            let serve_dir = tower_http::services::ServeDir::new(&web_path)
                .append_index_html_on_directories(true);
            app = app.fallback_service(serve_dir);
        }
    }

    // Add CORS for development
    let cors = tower_http::cors::CorsLayer::permissive();
    let app = app.layer(cors);

    let addr = format!("0.0.0.0:{}", port);
    println!("🪸 Grotto server listening on http://localhost:{}", port);
    println!("   WebSocket endpoint: ws://localhost:{}/ws", port);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_handler() -> impl IntoResponse {
    "ok"
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // Send snapshot on connect
    let snapshot = state.build_snapshot();
    if let Ok(json) = serde_json::to_string(&snapshot) {
        let _ = sender.send(Message::Text(json.into())).await;
    }

    // Subscribe to broadcast channel
    let mut rx = state.tx.subscribe();

    // Forward broadcast events to this WS client
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Read from client (just drain — we don't expect client messages)
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(_msg)) = receiver.next().await {
            // Client messages ignored for now
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = &mut send_task => { recv_task.abort(); }
        _ = &mut recv_task => { send_task.abort(); }
    }
}

async fn run_file_watcher(
    grotto_dir: PathBuf,
    tx: broadcast::Sender<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel(256);

    // Track last known event count for incremental reading
    let event_line_count = Arc::new(RwLock::new(count_lines(&grotto_dir.join("events.jsonl"))));

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = notify_tx.blocking_send(event);
            }
        },
        notify::Config::default(),
    )?;

    watcher.watch(&grotto_dir, RecursiveMode::Recursive)?;

    // Keep watcher alive
    let _watcher = watcher;

    while let Some(event) = notify_rx.recv().await {
        match event.kind {
            EventKind::Modify(_) | EventKind::Create(_) => {
                for path in &event.paths {
                    if let Some(fname) = path.file_name().and_then(|f| f.to_str()) {
                        match fname {
                            "events.jsonl" => {
                                let new_events = read_new_events(
                                    path,
                                    &event_line_count,
                                ).await;
                                for evt in new_events {
                                    let ws_event = WsEvent {
                                        event_type: "event:raw".to_string(),
                                        timestamp: evt.timestamp.to_rfc3339(),
                                        agent_id: evt.agent_id,
                                        task_id: evt.task_id,
                                        message: evt.message,
                                        data: Some(evt.data),
                                        agents: None,
                                        tasks: None,
                                        config: None,
                                    };
                                    if let Ok(json) = serde_json::to_string(&ws_event) {
                                        let _ = tx.send(json);
                                    }
                                }
                            }
                            "status.json" => {
                                // Agent status changed
                                if let Ok(content) = tokio::fs::read_to_string(path).await {
                                    if let Ok(agent) = serde_json::from_str::<AgentState>(&content) {
                                        let ws_event = WsEvent {
                                            event_type: "agent:status".to_string(),
                                            timestamp: chrono::Utc::now().to_rfc3339(),
                                            agent_id: Some(agent.id.clone()),
                                            task_id: agent.current_task.clone(),
                                            message: Some(format!(
                                                "Agent {} is now {}",
                                                agent.id, agent.state
                                            )),
                                            data: Some(serde_json::to_value(&agent).unwrap_or_default()),
                                            agents: None,
                                            tasks: None,
                                            config: None,
                                        };
                                        if let Ok(json) = serde_json::to_string(&ws_event) {
                                            let _ = tx.send(json);
                                        }
                                    }
                                }
                            }
                            "tasks.md" => {
                                // Task board changed — parse and broadcast
                                let tasks = parse_task_board(path);
                                let ws_event = WsEvent {
                                    event_type: "task:updated".to_string(),
                                    timestamp: chrono::Utc::now().to_rfc3339(),
                                    agent_id: None,
                                    task_id: None,
                                    message: Some("Task board updated".to_string()),
                                    data: Some(serde_json::to_value(&tasks).unwrap_or_default()),
                                    agents: None,
                                    tasks: Some(tasks),
                                    config: None,
                                };
                                if let Ok(json) = serde_json::to_string(&ws_event) {
                                    let _ = tx.send(json);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn count_lines(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|c| c.lines().count())
        .unwrap_or(0)
}

async fn read_new_events(
    path: &Path,
    line_count: &Arc<RwLock<usize>>,
) -> Vec<Event> {
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let lines: Vec<&str> = content.lines().collect();
    let mut prev = line_count.write().await;
    let new_lines = if lines.len() > *prev {
        &lines[*prev..]
    } else {
        &[]
    };

    let mut events = Vec::new();
    for line in new_lines {
        if let Ok(event) = serde_json::from_str::<Event>(line) {
            events.push(event);
        }
    }

    *prev = lines.len();
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_task_board() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tasks.md");
        std::fs::write(
            &path,
            "# Task Board\n\n\
             ⭕ **task-1** - Build the API\n\n\
             🟡 **task-2** - Write tests\n   - Claimed by: agent-1\n\n\
             ✅ **task-3** - Setup CI\n\n",
        )
        .unwrap();

        let tasks = parse_task_board(&path);
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].id, "task-1");
        assert_eq!(tasks[0].status, "open");
        assert_eq!(tasks[0].claimed_by, None);
        assert_eq!(tasks[1].id, "task-2");
        assert_eq!(tasks[1].status, "claimed");
        assert_eq!(tasks[1].claimed_by, Some("agent-1".to_string()));
        assert_eq!(tasks[2].id, "task-3");
        assert_eq!(tasks[2].status, "completed");
    }

    #[test]
    fn test_parse_task_line() {
        let task = parse_task_line("**main** - Do the thing", "open").unwrap();
        assert_eq!(task.id, "main");
        assert_eq!(task.description, "Do the thing");
        assert_eq!(task.status, "open");
    }

    #[test]
    fn test_parse_task_line_no_description() {
        let task = parse_task_line("**main**", "open").unwrap();
        assert_eq!(task.id, "main");
    }

    #[test]
    fn test_parse_task_line_invalid() {
        assert!(parse_task_line("no bold markers", "open").is_none());
    }

    #[test]
    fn test_count_lines() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.jsonl");
        std::fs::write(&path, "line1\nline2\nline3\n").unwrap();
        assert_eq!(count_lines(&path), 3);
    }

    #[test]
    fn test_count_lines_missing_file() {
        assert_eq!(count_lines(Path::new("/nonexistent/file")), 0);
    }

    #[test]
    fn test_ws_event_serialization() {
        let event = WsEvent {
            event_type: "agent:status".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            agent_id: Some("agent-1".to_string()),
            task_id: None,
            message: Some("Agent started".to_string()),
            data: None,
            agents: None,
            tasks: None,
            config: None,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"agent:status\""));
        assert!(json.contains("agent-1"));
        // Verify None fields are skipped
        assert!(!json.contains("task_id"));
        assert!(!json.contains("\"data\""));
    }

    #[test]
    fn test_snapshot_with_grotto_state() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let _grotto = Grotto::new(&dir, 2, "test task".into()).unwrap();

        let state = AppState {
            tx: broadcast::channel(16).0,
            grotto_dir: dir.join(".grotto"),
        };

        let snapshot = state.build_snapshot();
        assert_eq!(snapshot.event_type, "snapshot");
        assert!(snapshot.agents.is_some());
        let agents = snapshot.agents.unwrap();
        assert_eq!(agents.len(), 2);
        assert!(snapshot.config.is_some());
        assert_eq!(snapshot.config.unwrap().agent_count, 2);
    }

    #[test]
    fn test_snapshot_without_grotto() {
        let tmp = TempDir::new().unwrap();
        let state = AppState {
            tx: broadcast::channel(16).0,
            grotto_dir: tmp.path().join(".grotto"),
        };

        let snapshot = state.build_snapshot();
        assert_eq!(snapshot.event_type, "snapshot");
        assert!(snapshot.agents.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_read_new_events_incremental() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("events.jsonl");

        // Write initial events
        let event1 = r#"{"timestamp":"2025-01-01T00:00:00Z","event_type":"test","agent_id":null,"task_id":null,"message":"first","data":{}}"#;
        std::fs::write(&path, format!("{}\n", event1)).unwrap();

        let line_count = Arc::new(RwLock::new(0usize));

        // First read should get 1 event
        let events = read_new_events(&path, &line_count).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, Some("first".to_string()));

        // Append another event
        let event2 = r#"{"timestamp":"2025-01-01T00:00:01Z","event_type":"test","agent_id":null,"task_id":null,"message":"second","data":{}}"#;
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut file, format!("{}\n", event2).as_bytes()).unwrap();

        // Second read should only get the new event
        let events = read_new_events(&path, &line_count).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, Some("second".to_string()));
    }
}
