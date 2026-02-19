use axum::{
    Json, Router,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use futures::{Sink, SinkExt, StreamExt};
use grotto_core::daemon::{self, SessionEntry, SessionRegistry};
use grotto_core::monitor::{self, AgentPhase};
use grotto_core::{AgentState, Event, Grotto};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

#[derive(rust_embed::Embed)]
// Use crate-local assets so packaged installs always include the UI bundle.
#[folder = "web/"]
struct WebAssets;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_status: Option<String>,
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

impl WsEvent {
    fn base(event_type: impl Into<String>, timestamp: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            timestamp: timestamp.into(),
            agent_id: None,
            task_id: None,
            message: None,
            data: None,
            agents: None,
            tasks: None,
            config: None,
            session_active: None,
            session_status: None,
        }
    }

    fn snapshot(
        message: impl Into<String>,
        agents: HashMap<String, AgentState>,
        tasks: Vec<TaskInfo>,
        config: Option<ConfigInfo>,
        session_active: bool,
        session_status: impl Into<String>,
    ) -> Self {
        let mut event = Self::base("snapshot", chrono::Utc::now().to_rfc3339());
        event.message = Some(message.into());
        event.agents = Some(agents);
        event.tasks = Some(tasks);
        event.config = config;
        event.session_active = Some(session_active);
        event.session_status = Some(session_status.into());
        event
    }

    fn message_event(
        event_type: impl Into<String>,
        timestamp: impl Into<String>,
        agent_id: Option<String>,
        task_id: Option<String>,
        message: Option<String>,
        data: Option<serde_json::Value>,
    ) -> Self {
        let mut event = Self::base(event_type, timestamp);
        event.agent_id = agent_id;
        event.task_id = task_id;
        event.message = message;
        event.data = data;
        event
    }
}

fn detect_session_liveness(session_id: &str, agent_count: usize) -> (bool, String) {
    let snapshots = monitor::capture_all_agents(session_id, agent_count);
    if snapshots.is_empty() {
        return (false, "completed".to_string());
    }

    let all_finished = snapshots
        .iter()
        .all(|s| s.phase == AgentPhase::Finished && s.raw_content.is_empty());

    if all_finished {
        (false, "completed".to_string())
    } else {
        (true, "live".to_string())
    }
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
    /// Dropping this handle stops the tmux monitor task
    _monitor_abort: Option<tokio::task::AbortHandle>,
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

    /// Ensure in-memory sessions exist for entries from SessionRegistry on disk.
    async fn sync_from_registry(&self) {
        let registry = SessionRegistry::load();
        let mut sessions = self.sessions.write().await;

        // Add sessions present in registry but missing in memory
        for (id, entry) in registry.sessions {
            if sessions.contains_key(&id) {
                continue;
            }
            let dir = PathBuf::from(&entry.dir);
            let grotto_dir = dir.join(".grotto");
            if !grotto_dir.exists() {
                continue;
            }

            let (tx, _rx) = broadcast::channel::<String>(256);
            let abort_handle = spawn_session_watcher(grotto_dir, tx.clone());
            let monitor_abort =
                spawn_tmux_monitor(entry.id.clone(), entry.agent_count, tx.clone());
            sessions.insert(
                id,
                LiveSession {
                    entry,
                    tx,
                    _watcher_abort: abort_handle,
                    _monitor_abort: Some(monitor_abort),
                },
            );
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
            let monitor_abort =
                spawn_tmux_monitor(entry.id.clone(), entry.agent_count, tx.clone());
            let mut sessions = state.sessions.write().await;
            sessions.insert(
                entry.id.clone(),
                LiveSession {
                    entry,
                    tx,
                    _watcher_abort: abort_handle,
                    _monitor_abort: Some(monitor_abort),
                },
            );
        }
    }

    fn build_snapshot_for(grotto_dir: &std::path::Path) -> WsEvent {
        let project_dir = grotto_dir.parent().unwrap_or(std::path::Path::new("."));
        let grotto = Grotto::load(project_dir);

        match grotto {
            Ok(g) => {
                let tasks = parse_task_board(&grotto_dir.join("tasks.md"));

                // Enrich agents with live tmux phase data
                let mut agents = g.agents;
                let mut session_active = false;
                let mut session_status = "completed".to_string();

                if let Some(session_id) = &g.config.session_id {
                    let snapshots = monitor::capture_all_agents(session_id, g.config.agent_count);
                    for snap in &snapshots {
                        if let Some(agent) = agents.get_mut(&snap.agent_id) {
                            agent.phase = Some(snap.phase.to_string());
                        }
                    }

                    let (active, status) = detect_session_liveness(session_id, g.config.agent_count);
                    session_active = active;
                    session_status = status;
                }

                WsEvent::snapshot(
                    "Full state snapshot",
                    agents,
                    tasks,
                    Some(ConfigInfo {
                        agent_count: g.config.agent_count,
                        task: g.config.task.clone(),
                        project_dir: g.config.project_dir.display().to_string(),
                    }),
                    session_active,
                    session_status,
                )
            }
            Err(_) => WsEvent::snapshot(
                "No grotto state found",
                HashMap::new(),
                Vec::new(),
                None,
                false,
                "not_found",
            ),
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

pub async fn run_server(
    grotto_dir: PathBuf,
    port: u16,
    web_dir: Option<PathBuf>,
) -> std::io::Result<()> {
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

    // Start tmux monitor for real-time phase tracking
    let project_dir = grotto_dir.parent().unwrap_or(std::path::Path::new("."));
    if let Ok(g) = Grotto::load(project_dir) {
        if let Some(session_id) = &g.config.session_id {
            let _monitor = spawn_tmux_monitor(session_id.clone(), g.config.agent_count, tx.clone());
        }
    }

    let mut app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_handler))
        .with_state(state);

    if let Some(web_path) = web_dir
        && web_path.exists() {
            let serve_dir = tower_http::services::ServeDir::new(&web_path)
                .append_index_html_on_directories(true);
            app = app.fallback_service(serve_dir);
        } else {
            app = app.fallback(serve_embedded);
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

async fn forward_broadcast_to_ws<S>(mut sender: S, mut rx: broadcast::Receiver<String>)
where
    S: Sink<Message> + Unpin,
{
    loop {
        match rx.recv().await {
            Ok(msg) => {
                if sender.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                eprintln!("ws receiver lagged; skipped {skipped} messages");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn handle_ws(socket: WebSocket, tx: broadcast::Sender<String>, grotto_dir: PathBuf) {
    let (mut sender, mut receiver) = socket.split();

    // Send snapshot on connect
    let snapshot = DaemonState::build_snapshot_for(&grotto_dir);
    if let Ok(json) = serde_json::to_string(&snapshot) {
        let _ = sender.send(Message::Text(json.into())).await;
    }

    let rx = tx.subscribe();

    let mut send_task = tokio::spawn(async move {
        forward_broadcast_to_ws(sender, rx).await;
    });

    let mut recv_task =
        tokio::spawn(async move { while let Some(Ok(_msg)) = receiver.next().await {} });

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
    pub status: String,
    pub last_updated: Option<String>,
}

/// Run the multi-session daemon server
pub async fn run_daemon(port: u16, web_dir: Option<PathBuf>) -> std::io::Result<()> {
    daemon::write_pid(std::process::id())?;

    let state = Arc::new(DaemonState::new(web_dir));

    // Load any previously-registered sessions
    DaemonState::load_persisted(&state).await;

    let daemon_state = state.clone();

    let app = Router::new()
        .route(
            "/api/sessions",
            get({
                let s = daemon_state.clone();
                move || api_list_sessions(s)
            }),
        )
        .route(
            "/api/sessions",
            post({
                let s = daemon_state.clone();
                move |body| api_register_session(s, body)
            }),
        )
        .route(
            "/api/sessions/{id}",
            delete({
                let s = daemon_state.clone();
                move |path| api_unregister_session(s, path)
            }),
        )
        .route(
            "/api/sessions/{id}/events",
            get({
                let s = daemon_state.clone();
                move |path| api_session_events(s, path)
            }),
        )
        .route(
            "/ws/{id}",
            get({
                let s = daemon_state.clone();
                move |path, ws| daemon_ws_handler(s, path, ws)
            }),
        )
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

fn read_last_event_timestamp(session_dir: &str) -> Option<String> {
    let events_path = PathBuf::from(session_dir).join(".grotto").join("events.jsonl");
    let content = std::fs::read_to_string(&events_path).ok()?;
    let last_line = content.lines().filter(|line| !line.trim().is_empty()).next_back()?;
    let value: serde_json::Value = serde_json::from_str(last_line).ok()?;
    value
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn infer_session_status(entry: &SessionEntry) -> String {
    if entry.agent_count == 0 {
        return "completed".to_string();
    }
    let (_active, status) = detect_session_liveness(&entry.id, entry.agent_count);
    status
}

async fn api_list_sessions(state: Arc<DaemonState>) -> impl IntoResponse {
    state.sync_from_registry().await;
    let registry = SessionRegistry::load();
    let mut result: Vec<SessionResponse> = Vec::new();

    for session in registry.sessions.values() {
        result.push(SessionResponse {
            id: session.id.clone(),
            dir: session.dir.clone(),
            agent_count: Some(session.agent_count),
            task: Some(session.task.clone()),
            status: infer_session_status(session),
            last_updated: read_last_event_timestamp(&session.dir),
        });
    }

    result.sort_by(|a, b| b.last_updated.cmp(&a.last_updated).then_with(|| a.id.cmp(&b.id)));

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

    let monitor_abort = spawn_tmux_monitor(body.id.clone(), agent_count, tx.clone());

    let entry = SessionEntry {
        id: body.id.clone(),
        dir: body.dir.clone(),
        agent_count,
        task,
    };

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(
            body.id.clone(),
            LiveSession {
                entry,
                tx,
                _watcher_abort: abort_handle,
                _monitor_abort: Some(monitor_abort),
            },
        );
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

async fn api_session_events(state: Arc<DaemonState>, Path(id): Path<String>) -> impl IntoResponse {
    state.sync_from_registry().await;
    let registry = SessionRegistry::load();
    match registry.sessions.get(&id) {
        Some(session) => {
            let events_path = PathBuf::from(&session.dir)
                .join(".grotto")
                .join("events.jsonl");
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
    state.sync_from_registry().await;

    // Enforce registry as source of truth for route validity.
    let registry = SessionRegistry::load();
    if !registry.sessions.contains_key(&id) {
        return (
            axum::http::StatusCode::NOT_FOUND,
            format!("Session '{}' not found", id),
        )
            .into_response();
    }

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
    state.sync_from_registry().await;
    let path = req.uri().path();

    let web_dir = state.web_dir.as_ref();

    // Root → serve index.html
    if path == "/" {
        return serve_file_or_embedded(web_dir, "index.html").await;
    }

    let segment = path.trim_start_matches('/');

    // Named HTML pages (no extension in URL)
    let html_pages = ["sandbox", "crab-styles"];
    for page in &html_pages {
        if segment == *page {
            let filename = format!("{}.html", page);
            return serve_file_or_embedded(web_dir, &filename).await;
        }
    }

    // Static files — anything with a file extension (supports subdirs)
    if segment.contains('.') {
        return serve_file_or_embedded(web_dir, segment).await;
    }

    // Session route — serve session.html for registered session IDs
    let registry = SessionRegistry::load();
    if registry.sessions.contains_key(segment) {
        return serve_file_or_embedded(web_dir, "session.html").await;
    }

    (axum::http::StatusCode::NOT_FOUND, "not found").into_response()
}

/// Serve a file from disk (if web_dir is set and file exists), otherwise from embedded assets.
async fn serve_file_or_embedded(web_dir: Option<&PathBuf>, rel_path: &str) -> Response {
    // Try disk first
    if let Some(dir) = web_dir {
        let file_path = dir.join(rel_path);
        if let Ok(content) = tokio::fs::read(&file_path).await {
            let mime = mime_from_ext(&file_path);
            return ([(axum::http::header::CONTENT_TYPE, mime)], content).into_response();
        }
    }

    // Fall back to embedded
    serve_embedded_file(rel_path)
}

/// Serve a single file from embedded assets.
fn serve_embedded_file(rel_path: &str) -> Response {
    match WebAssets::get(rel_path) {
        Some(file) => {
            let mime = mime_from_ext(std::path::Path::new(rel_path));
            (
                [(axum::http::header::CONTENT_TYPE, mime)],
                file.data.to_vec(),
            )
                .into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// Axum fallback handler that serves from embedded assets (used by single-session server).
async fn serve_embedded(req: axum::http::Request<axum::body::Body>) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    serve_embedded_file(path)
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
// Tmux pane monitor (spawned as a tokio task)
// ---------------------------------------------------------------------------

/// Spawn a tmux monitor that polls pane output every 750ms and broadcasts
/// `agent:phase` events when an agent's phase changes.
fn spawn_tmux_monitor(
    session_id: String,
    agent_count: usize,
    tx: broadcast::Sender<String>,
) -> tokio::task::AbortHandle {
    let handle = tokio::spawn(async move {
        run_tmux_monitor(session_id, agent_count, tx).await;
    });
    handle.abort_handle()
}

async fn run_tmux_monitor(
    session_id: String,
    agent_count: usize,
    tx: broadcast::Sender<String>,
) {
    use std::collections::HashMap;

    let mut prev_phases: HashMap<String, AgentPhase> = HashMap::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(750));

    // Track consecutive capture failures to detect session death
    let mut consecutive_failures: usize = 0;

    loop {
        interval.tick().await;

        let snapshots = tokio::task::spawn_blocking({
            let session_id = session_id.clone();
            move || monitor::capture_all_agents(&session_id, agent_count)
        })
        .await
        .unwrap_or_default();

        if snapshots.is_empty() {
            continue;
        }

        // Check if all panes failed to capture (session likely dead)
        let all_finished = snapshots
            .iter()
            .all(|s| s.phase == AgentPhase::Finished && s.raw_content.is_empty());
        if all_finished {
            consecutive_failures += 1;
            if consecutive_failures > 5 {
                let now = chrono::Utc::now();
                let mut ws_event = WsEvent::message_event(
                    "session:completed",
                    now.to_rfc3339(),
                    None,
                    None,
                    Some("Session completed (tmux session ended)".to_string()),
                    Some(serde_json::json!({
                        "reason": "tmux_session_ended",
                        "completed_at": now.to_rfc3339(),
                        "session_id": session_id,
                    })),
                );
                ws_event.session_active = Some(false);
                ws_event.session_status = Some("completed".to_string());

                if let Ok(json) = serde_json::to_string(&ws_event) {
                    let _ = tx.send(json);
                }

                // Session is gone, stop polling
                break;
            }
        } else {
            consecutive_failures = 0;
        }

        for snap in &snapshots {
            let changed = prev_phases
                .get(&snap.agent_id)
                .map(|prev| *prev != snap.phase)
                .unwrap_or(true);

            if changed {
                prev_phases.insert(snap.agent_id.clone(), snap.phase.clone());

                let ws_event = WsEvent::message_event(
                    "agent:phase",
                    snap.timestamp.to_rfc3339(),
                    Some(snap.agent_id.clone()),
                    None,
                    Some(format!("Agent {} phase: {}", snap.agent_id, snap.phase)),
                    Some(serde_json::json!({
                        "phase": snap.phase.to_string(),
                        "last_activity": snap.last_activity_line,
                    })),
                );

                if let Ok(json) = serde_json::to_string(&ws_event) {
                    let _ = tx.send(json);
                }
            }
        }
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
                                let new_events = read_new_events(path, &event_line_count).await;
                                for evt in new_events {
                                    let ws_event = WsEvent::message_event(
                                        "event:raw",
                                        evt.timestamp.to_rfc3339(),
                                        evt.agent_id,
                                        evt.task_id,
                                        evt.message,
                                        Some(evt.data),
                                    );
                                    if let Ok(json) = serde_json::to_string(&ws_event) {
                                        let _ = tx.send(json);
                                    }
                                }
                            }
                            "status.json" => {
                                if let Ok(content) = tokio::fs::read_to_string(path).await
                                    && let Ok(agent) = serde_json::from_str::<AgentState>(&content)
                                    {
                                        let ws_event = WsEvent::message_event(
                                            "agent:status",
                                            chrono::Utc::now().to_rfc3339(),
                                            Some(agent.id.clone()),
                                            agent.current_task.clone(),
                                            Some(format!("Agent {} is now {}", agent.id, agent.state)),
                                            Some(serde_json::to_value(&agent).unwrap_or_default()),
                                        );
                                        if let Ok(json) = serde_json::to_string(&ws_event) {
                                            let _ = tx.send(json);
                                        }
                                    }
                            }
                            "tasks.md" => {
                                let tasks = parse_task_board(path);
                                let mut ws_event = WsEvent::message_event(
                                    "task:updated",
                                    chrono::Utc::now().to_rfc3339(),
                                    None,
                                    None,
                                    Some("Task board updated".to_string()),
                                    Some(serde_json::to_value(&tasks).unwrap_or_default()),
                                );
                                ws_event.tasks = Some(tasks);
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

async fn read_new_events(path: &std::path::Path, line_count: &Arc<RwLock<usize>>) -> Vec<Event> {
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
    use futures::{channel::mpsc, StreamExt};
    use std::time::Duration;
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
            session_active: None,
            session_status: None,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"agent:status\""));
        assert!(json.contains("agent-1"));
        assert!(!json.contains("task_id"));
        assert!(!json.contains("\"data\""));
    }

    #[test]
    fn test_message_event_helper_serialization() {
        let event = WsEvent::message_event(
            "event:raw",
            "2025-01-01T00:00:00Z",
            Some("agent-1".to_string()),
            None,
            None,
            Some(serde_json::json!({"ok": true})),
        );

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"event:raw\""));
        assert!(json.contains("agent-1"));
        assert!(!json.contains("\"message\""));
    }

    #[test]
    fn test_session_completed_event_serialization() {
        let event = WsEvent {
            event_type: "session:completed".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            agent_id: None,
            task_id: None,
            message: Some("Session completed (tmux session ended)".to_string()),
            data: Some(serde_json::json!({"reason": "tmux_session_ended"})),
            agents: None,
            tasks: None,
            config: None,
            session_active: Some(false),
            session_status: Some("completed".to_string()),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"session:completed\""));
        assert!(json.contains("\"session_active\":false"));
        assert!(json.contains("\"session_status\":\"completed\""));
    }

    #[test]
    fn test_web_app_history_mode_does_not_complete_session() {
        let app_js = include_str!("../web/app.js");

        assert!(
            app_js.contains("case 'session:completed':\n        if (!options.fromHistory) {")
                || app_js.contains("case 'session:completed':\r\n        if (!options.fromHistory) {"),
            "session:completed handler must ignore history replay events"
        );
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
        assert!(snapshot.session_active.is_some());
        assert!(snapshot.session_status.is_some());
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
        assert_eq!(snapshot.session_active, Some(false));
        assert_eq!(snapshot.session_status, Some("not_found".to_string()));
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
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut file, format!("{}\n", event2).as_bytes()).unwrap();

        let events = read_new_events(&path, &line_count).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, Some("second".to_string()));
    }

    #[tokio::test]
    async fn test_forward_broadcast_survives_lagged() {
        let (tx, rx) = broadcast::channel::<String>(2);
        let (sink_tx, mut sink_rx) = mpsc::channel::<Message>(1);

        let forward = tokio::spawn(async move {
            forward_broadcast_to_ws(sink_tx, rx).await;
        });

        tx.send("first".to_string()).unwrap();
        tokio::task::yield_now().await;

        tx.send("second".to_string()).unwrap();
        tokio::task::yield_now().await;

        for i in 0..5 {
            let _ = tx.send(format!("spam-{i}"));
        }

        let _ = tokio::time::timeout(Duration::from_secs(1), sink_rx.next())
            .await
            .expect("timeout waiting for first message");
        let _ = tokio::time::timeout(Duration::from_secs(1), sink_rx.next())
            .await
            .expect("timeout waiting for second message");

        tx.send("after".to_string()).unwrap();

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(msg) = sink_rx.next().await {
                    if let Message::Text(text) = msg {
                        if text == "after" {
                            break;
                        }
                    }
                }
            }
        })
        .await
        .expect("timeout waiting for post-lag message");

        drop(tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), forward)
            .await
            .expect("forward task did not finish");
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
            sessions.insert(
                "test-session".to_string(),
                LiveSession {
                    entry: SessionEntry {
                        id: "test-session".to_string(),
                        dir: project_dir.display().to_string(),
                        agent_count: 2,
                        task: "test".to_string(),
                    },
                    tx,
                    _watcher_abort: abort_handle,
                    _monitor_abort: None,
                },
            );
        }

        state.save().await;

        // Verify the registry was persisted
        let registry = SessionRegistry::load();
        assert_eq!(registry.sessions.len(), 1);
        assert!(registry.sessions.contains_key("test-session"));
    }

    #[test]
    fn test_embedded_web_assets_present() {
        // Regression guard: bad embed paths cause UI white-screen/404.
        assert!(WebAssets::get("index.html").is_some());
        assert!(WebAssets::get("app.js").is_some());
    }

    #[test]
    fn test_mime_from_ext() {
        assert_eq!(
            mime_from_ext(std::path::Path::new("style.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(
            mime_from_ext(std::path::Path::new("app.js")),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(
            mime_from_ext(std::path::Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            mime_from_ext(std::path::Path::new("data.json")),
            "application/json"
        );
        assert_eq!(
            mime_from_ext(std::path::Path::new("image.png")),
            "image/png"
        );
        assert_eq!(
            mime_from_ext(std::path::Path::new("unknown.xyz")),
            "application/octet-stream"
        );
    }
}
