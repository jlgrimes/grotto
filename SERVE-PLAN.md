# grotto serve — Real-time WebSocket Server + Web UI

## Overview
Add a `grotto serve` command that watches `.grotto/` for file changes and broadcasts events over WebSocket. Also serves a static web UI with pixel art crabs.

## Architecture
```
Agents → write .grotto/ files (events.jsonl, agents/*/status.json)
              ↓
       grotto serve (file watcher + WS broadcast + static file server)
              ↓
    ┌─────────┴─────────┐
    CLI (reads files)    Browser (WS client at ws://localhost:9090/ws)
```

## Event Schema (JSON over WebSocket)
All events have this shape:
```json
{
  "type": "agent:status" | "task:claimed" | "task:completed" | "team:spawned" | "agent:summary" | "event:raw",
  "timestamp": "2025-01-01T00:00:00Z",
  "agent_id": "agent-1" | null,
  "task_id": "main" | null,
  "message": "human readable description",
  "data": { ... }  // event-specific payload
}
```

### Event Types
- `team:spawned` — new grotto session initialized
- `agent:status` — agent status.json changed (state, progress, current_task)
- `task:claimed` — task claimed by agent
- `task:completed` — task marked done
- `agent:summary` — agent wrote a completion summary
- `event:raw` — new line appended to events.jsonl (passthrough)
- `snapshot` — full state dump (sent on WS connect)

### Snapshot (sent on connect)
```json
{
  "type": "snapshot",
  "agents": { "agent-1": { ...AgentState }, ... },
  "tasks": [ { ...Task }, ... ],
  "config": { "agent_count": 3, "task": "...", "project_dir": "..." }
}
```

## Implementation

### New crate: `crates/grotto-serve/`
Dependencies:
- `axum` — HTTP server + static files
- `tokio` — async runtime
- `tokio-tungstenite` — WebSocket
- `notify` (v7) — file system watcher
- `tower-http` — static file serving
- `grotto-core` — reuse existing types

### Key components:
1. **FileWatcher** — watches `.grotto/` recursively using `notify` crate
   - On `events.jsonl` modify → read new lines, parse, broadcast
   - On `agents/*/status.json` modify → read, diff, broadcast `agent:status`
   - On `tasks.md` modify → could parse but easier to watch events.jsonl

2. **WsBroadcast** — tokio broadcast channel
   - New WS connections get a `snapshot` immediately
   - Then receive events as they happen

3. **Static file server** — serves `web/` directory (the pixi.js frontend)

4. **CLI integration** — `grotto serve` command with options:
   - `--port 9090` (default)
   - `--dir <project-dir>` (default: cwd)
   - `--no-open` (don't auto-open browser)

### Web Frontend: `web/`
- `index.html` — single page
- `app.js` — WS client + pixi.js rendering
- `style.css` — minimal layout
- Pixel art assets in `web/assets/`

### Crab UI Behavior
- Each agent = a pixel art crab
- States:
  - **idle** → crab walking slowly, looking around
  - **spawning** → crab emerging from sand
  - **working** → crab hammering/building (animated)
  - **completed** → crab does a little dance, then idle
- Task board shown as a coral reef bulletin board
- Events scroll in a log panel at bottom
- Agent name labels above each crab

## File Changes
1. `Cargo.toml` (workspace) — add `crates/grotto-serve` member
2. `crates/grotto-serve/Cargo.toml` — new crate
3. `crates/grotto-serve/src/main.rs` or `lib.rs` — server logic
4. `crates/grotto-cli/Cargo.toml` — add grotto-serve dep (or make serve its own binary)
5. `crates/grotto-cli/src/main.rs` — add `Serve` command
6. `web/` — frontend files

## Task Breakdown
- **Task 1**: Create `grotto-serve` crate with axum WS server + file watcher (backend only, no frontend)
- **Task 2**: Build pixi.js web frontend with crab animations
- **Task 3**: Integration — wire serve into CLI, test end-to-end, write tests

## Research First (Stand on Giants)
- Check `notify` crate v7 API for recursive watching
- Check `axum` WebSocket example
- Check pixi.js sprite animation examples
- Find or create pixel art crab sprites (can use simple placeholder first)
