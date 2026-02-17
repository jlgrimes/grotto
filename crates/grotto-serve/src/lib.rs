use axum::{
    Router,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
    Json,
};
use futures::{SinkExt, StreamExt};
use grotto_core::{AgentState, Event, Grotto};
use grotto_core::daemon::{self, SessionEntry, SessionRegistry};
use notify::{RecommendedWatcher, RecursiveMode, Watcher, EventKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

// ---------------------------------------------------------------------------
// Public types (kept backward-compatible)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Session registry types
// ---------------------------------------------------------------------------

/// Runtime state for a live session (in-memory, per-session broadcast + watcher)
struct LiveSession {
    pub entry: SessionEntry,
    pub tx: broadcast::Sender<String>,
    /// Dropping this handle stops the watcher task
    _watcher_abort: tokio::task::AbortHandle,
}

/// Shared daemon state
pub struct DaemonState {
    sessions: RwLock<HashMap<String, LiveSession>>,
    web_dir: Option<PathBuf>,
}

impl DaemonState {
    fn new(web_dir: Option<PathBuf>) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            web_dir,
        }
    }

    /// Persist current session list to the SessionRegistry on disk
    async fn save(&self) {
        let sessions = self.sessions.read().await;
        let mut registry = SessionRegistry::default();
        for session in sessions.values() {
            registry.register(session.entry.clone());
        }
        let _ = registry.save();
    }

    /// Load sessions from the SessionRegistry on disk and start file watchers
    async fn load_persisted(state: &Arc<DaemonState>) {
        let registry = SessionRegistry::load();
        for (_, entry) in registry.sessions {
            let dir = PathBuf::from(&entry.dir);
            let grotto_dir = dir.join(".grotto");
            if !grotto_dir.exists() {
                continue; // stale session, skip
            }
            let (tx, _rx) = broadcast::channel::<String>(256);
            let abort_handle = spawn_session_watcher(grotto_dir, tx.clone());
            let mut sessions = state.sessions.write().await;
            sessions.insert(entry.id.clone(), LiveSession {
                entry,
                tx,
                _watcher_abort: abort_handle,
            });
        }
    }

    fn build_snapshot_for(grotto_dir: &std::path::Path) -> WsEvent {
        let project_dir = grotto_dir.parent().unwrap_or(std::path::Path::new("."));
        let grotto = Grotto::load(project_dir);

        match grotto {
            Ok(g) => {
                let tasks = parse_task_board(&grotto_dir.join("tasks.md"));
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

// Keep the old AppState for backward-compat with the single-session `run_server`
#[derive(Clone)]
pub struct AppState {
    pub tx: broadcast::Sender<String>,
    pub grotto_dir: PathBuf,
}

impl AppState {
    pub fn build_snapshot(&self) -> WsEvent {
        DaemonState::build_snapshot_for(&self.grotto_dir)
    }
}

// ---------------------------------------------------------------------------
// Task board parsing (unchanged)
// ---------------------------------------------------------------------------

fn parse_task_board(path: &std::path::Path) -> Vec<TaskInfo> {
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

// ---------------------------------------------------------------------------
// Single-session server (backward compatible — used by `grotto serve`)
// ---------------------------------------------------------------------------

pub async fn run_server(grotto_dir: PathBuf, port: u16, web_dir: Option<PathBuf>) -> std::io::Result<()> {
    let (tx, _rx) = broadcast::channel::<String>(256);
    let state = AppState {
        tx: tx.clone(),
        grotto_dir: grotto_dir.clone(),
    };

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

    if let Some(web_path) = web_dir {
        if web_path.exists() {
            let serve_dir = tower_http::services::ServeDir::new(&web_path)
                .append_index_html_on_directories(true);
            app = app.fallback_service(serve_dir);
        }
    }

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
    ws.on_upgrade(move |socket| handle_ws(socket, state.tx.clone(), state.grotto_dir.clone()))
}

async fn handle_ws(socket: WebSocket, tx: broadcast::Sender<String>, grotto_dir: PathBuf) {
    let (mut sender, mut receiver) = socket.split();

    // Send snapshot on connect
    let snapshot = DaemonState::build_snapshot_for(&grotto_dir);
    if let Ok(json) = serde_json::to_string(&snapshot) {
        let _ = sender.send(Message::Text(json.into())).await;
    }

    let mut rx = tx.subscribe();

    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(_msg)) = receiver.next().await {}
    });

    tokio::select! {
        _ = &mut send_task => { recv_task.abort(); }
        _ = &mut recv_task => { send_task.abort(); }
    }
}

// ---------------------------------------------------------------------------
// Multi-session daemon server
// ---------------------------------------------------------------------------

/// API request body for registering a session
#[derive(Debug, Deserialize)]
pub struct RegisterSession {
    pub id: String,
    pub dir: String,
}

/// API response for session info
#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub id: String,
    pub dir: String,
    pub agent_count: Option<usize>,
    pub task: Option<String>,
}

/// Run the multi-session daemon server
pub async fn run_daemon(port: u16, web_dir: Option<PathBuf>) -> std::io::Result<()> {
    daemon::write_pid(std::process::id())?;

    let state = Arc::new(DaemonState::new(web_dir));

    // Load any previously-registered sessions
    DaemonState::load_persisted(&state).await;

    let daemon_state = state.clone();

    let app = Router::new()
        .route("/api/sessions", get({
            let s = daemon_state.clone();
            move || api_list_sessions(s)
        }))
        .route("/api/sessions", post({
            let s = daemon_state.clone();
            move |body| api_register_session(s, body)
        }))
        .route("/api/sessions/{id}", delete({
            let s = daemon_state.clone();
            move |path| api_unregister_session(s, path)
        }))
        .route("/api/sessions/{id}/events", get({
            let s = daemon_state.clone();
            move |path| api_session_events(s, path)
        }))
        .route("/ws/{id}", get({
            let s = daemon_state.clone();
            move |path, ws| daemon_ws_handler(s, path, ws)
        }))
        .route("/health", get(health_handler))
        .fallback({
            let s = daemon_state.clone();
            move |req: axum::http::Request<axum::body::Body>| daemon_fallback(s, req)
        })
        .layer(tower_http::cors::CorsLayer::permissive());

    let addr = format!("0.0.0.0:{}", port);
    println!("🪸 Grotto daemon listening on http://0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    // Clean up PID file on shutdown
    let result = axum::serve(listener, app).await;
    let _ = daemon::remove_pid();
    result
}

// ---------------------------------------------------------------------------
// REST API handlers
// ---------------------------------------------------------------------------

async fn api_list_sessions(state: Arc<DaemonState>) -> impl IntoResponse {
    let sessions = state.sessions.read().await;
    let mut result: Vec<SessionResponse> = Vec::new();

    for session in sessions.values() {
        result.push(SessionResponse {
            id: session.entry.id.clone(),
            dir: session.entry.dir.clone(),
            agent_count: Some(session.entry.agent_count),
            task: Some(session.entry.task.clone()),
        });
    }

    Json(result)
}

async fn api_register_session(
    state: Arc<DaemonState>,
    Json(body): Json<RegisterSession>,
) -> impl IntoResponse {
    let dir = PathBuf::from(&body.dir);
    let grotto_dir = dir.join(".grotto");

    if !grotto_dir.exists() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "No .grotto directory found at specified path"})),
        );
    }

    let (tx, _rx) = broadcast::channel::<String>(256);
    let abort_handle = spawn_session_watcher(grotto_dir, tx.clone());

    // Read task info from the grotto state
    let (agent_count, task) = match Grotto::load(&dir) {
        Ok(g) => (g.config.agent_count, g.config.task),
        Err(_) => (0, String::new()),
    };

    let entry = SessionEntry {
        id: body.id.clone(),
        dir: body.dir.clone(),
        agent_count,
        task,
    };

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(body.id.clone(), LiveSession {
            entry,
            tx,
            _watcher_abort: abort_handle,
        });
    }

    state.save().await;

    (
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({"id": body.id, "status": "registered"})),
    )
}

async fn api_unregister_session(
    state: Arc<DaemonState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut sessions = state.sessions.write().await;
    if sessions.remove(&id).is_some() {
        drop(sessions);
        state.save().await;
        (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({"id": id, "status": "unregistered"})),
        )
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Session '{}' not found", id)})),
        )
    }
}

async fn api_session_events(
    state: Arc<DaemonState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let sessions = state.sessions.read().await;
    match sessions.get(&id) {
        Some(session) => {
            let events_path = PathBuf::from(&session.entry.dir).join(".grotto").join("events.jsonl");
            let content = std::fs::read_to_string(events_path).unwrap_or_default();
            let events: Vec<serde_json::Value> = content
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect();
            (axum::http::StatusCode::OK, Json(serde_json::json!(events)))
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Session '{}' not found", id)})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Per-session WebSocket handler
// ---------------------------------------------------------------------------

async fn daemon_ws_handler(
    state: Arc<DaemonState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let sessions = state.sessions.read().await;
    match sessions.get(&id) {
        Some(session) => {
            let tx = session.tx.clone();
            let grotto_dir = PathBuf::from(&session.entry.dir).join(".grotto");
            drop(sessions);
            ws.on_upgrade(move |socket| handle_ws(socket, tx, grotto_dir))
                .into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            format!("Session '{}' not found", id),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Daemon fallback — serves index, session pages, and static files
// ---------------------------------------------------------------------------

/// Custom fallback: `/` → index.html, `/:session-id` → session.html, else static file
async fn daemon_fallback(
    state: Arc<DaemonState>,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    let path = req.uri().path();

    let Some(web_dir) = &state.web_dir else {
        return (axum::http::StatusCode::NOT_FOUND, "no web directory configured").into_response();
    };

    // Root → serve index.html
    if path == "/" {
        return match tokio::fs::read_to_string(web_dir.join("index.html")).await {
            Ok(html) => Html(html).into_response(),
            Err(_) => (axum::http::StatusCode::NOT_FOUND, "index.html not found").into_response(),
        };
    }

    let segment = path.trim_start_matches('/');

    // Static files — anything with a file extension
    if segment.contains('.') {
        let file_path = web_dir.join(segment);
        return match tokio::fs::read(&file_path).await {
            Ok(content) => {
                let mime = mime_from_ext(&file_path);
                ([(axum::http::header::CONTENT_TYPE, mime)], content).into_response()
            }
            Err(_) => (axum::http::StatusCode::NOT_FOUND, "file not found").into_response(),
        };
    }

    // Session route — serve session.html for registered session IDs
    let sessions = state.sessions.read().await;
    let is_session = sessions.contains_key(segment);
    drop(sessions);

    if is_session {
        return match tokio::fs::read_to_string(web_dir.join("session.html")).await {
            Ok(html) => Html(html).into_response(),
            Err(_) => (axum::http::StatusCode::NOT_FOUND, "session.html not found").into_response(),
        };
    }

    (axum::http::StatusCode::NOT_FOUND, "not found").into_response()
}

fn mime_from_ext(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// Per-session file watcher (spawned as a tokio task)
// ---------------------------------------------------------------------------

/// Spawn a file watcher for a session's .grotto/ directory.
/// Returns an AbortHandle — dropping it stops the watcher task.
fn spawn_session_watcher(
    grotto_dir: PathBuf,
    tx: broadcast::Sender<String>,
) -> tokio::task::AbortHandle {
    let handle = tokio::spawn(async move {
        if let Err(e) = run_file_watcher(grotto_dir, tx).await {
            eprintln!("File watcher error: {}", e);
        }
    });
    handle.abort_handle()
}

async fn run_file_watcher(
    grotto_dir: PathBuf,
    tx: broadcast::Sender<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel(256);

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

fn count_lines(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path)
        .map(|c| c.lines().count())
        .unwrap_or(0)
}

async fn read_new_events(
    path: &std::path::Path,
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
        assert_eq!(count_lines(std::path::Path::new("/nonexistent/file")), 0);
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

        let event1 = r#"{"timestamp":"2025-01-01T00:00:00Z","event_type":"test","agent_id":null,"task_id":null,"message":"first","data":{}}"#;
        std::fs::write(&path, format!("{}\n", event1)).unwrap();

        let line_count = Arc::new(RwLock::new(0usize));

        let events = read_new_events(&path, &line_count).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, Some("first".to_string()));

        let event2 = r#"{"timestamp":"2025-01-01T00:00:01Z","event_type":"test","agent_id":null,"task_id":null,"message":"second","data":{}}"#;
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut file, format!("{}\n", event2).as_bytes()).unwrap();

        let events = read_new_events(&path, &line_count).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, Some("second".to_string()));
    }

    #[test]
    fn test_session_entry_serialization() {
        let entry = SessionEntry {
            id: "crimson-coral-tide".to_string(),
            dir: "/home/user/project".to_string(),
            agent_count: 3,
            task: "build stuff".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("crimson-coral-tide"));
        let back: SessionEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "crimson-coral-tide");
        assert_eq!(back.agent_count, 3);
    }

    #[tokio::test]
    async fn test_daemon_state_save_load() {
        let tmp = TempDir::new().unwrap();

        let state = Arc::new(DaemonState::new(None));

        // Register a mock session
        let project_dir = tmp.path().join("project1");
        let _grotto = Grotto::new(&project_dir, 2, "test".into()).unwrap();
        let grotto_dir = project_dir.join(".grotto");

        let (tx, _rx) = broadcast::channel::<String>(16);
        let abort_handle = spawn_session_watcher(grotto_dir, tx.clone());

        {
            let mut sessions = state.sessions.write().await;
            sessions.insert("test-session".to_string(), LiveSession {
                entry: SessionEntry {
                    id: "test-session".to_string(),
                    dir: project_dir.display().to_string(),
                    agent_count: 2,
                    task: "test".to_string(),
                },
                tx,
                _watcher_abort: abort_handle,
            });
        }

        state.save().await;

        // Verify the registry was persisted
        let registry = SessionRegistry::load();
        assert_eq!(registry.sessions.len(), 1);
        assert!(registry.sessions.contains_key("test-session"));
    }

    #[test]
    fn test_mime_from_ext() {
        assert_eq!(mime_from_ext(std::path::Path::new("style.css")), "text/css; charset=utf-8");
        assert_eq!(mime_from_ext(std::path::Path::new("app.js")), "application/javascript; charset=utf-8");
        assert_eq!(mime_from_ext(std::path::Path::new("index.html")), "text/html; charset=utf-8");
        assert_eq!(mime_from_ext(std::path::Path::new("data.json")), "application/json");
        assert_eq!(mime_from_ext(std::path::Path::new("image.png")), "image/png");
        assert_eq!(mime_from_ext(std::path::Path::new("unknown.xyz")), "application/octet-stream");
    }
}
