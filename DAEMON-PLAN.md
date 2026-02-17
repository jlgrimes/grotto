# grotto daemon — Persistent Multi-Session Server

## Overview
Convert `grotto serve` into a long-running daemon that manages multiple project sessions. Each session gets a unique semantic ID and its own route in the web UI.

## Semantic IDs
Generate `adjective-noun-noun` IDs for sessions (like Docker/GH Codespaces):
- Build small word lists into the binary (~50 adjectives, ~50 nouns)
- Generate on `grotto spawn`, store in `.grotto/config.toml` as `session_id`
- Used as the URL route: `http://host:9091/crimson-coral-tide`

## Daemon Commands
```
grotto daemon start     # Start daemon on port 9091 (background, writes PID file)
grotto daemon stop      # Kill daemon
grotto daemon status    # Check if running
```

PID file: `~/.grotto/daemon.pid`
State file: `~/.grotto/sessions.json` (registered sessions)

## API
```
GET  /api/sessions                  # List all sessions
POST /api/sessions                  # Register { dir: "...", id: "..." }
DELETE /api/sessions/:id            # Unregister
GET  /api/sessions/:id/events       # Event history
WS   /ws/:id                        # Real-time events for one session
```

## Routes (Web UI)
```
GET /                    # Index — list of session links
GET /:session-id         # Session page — crabs, task board, event log
```

- Index page: simple list of active sessions with links, project dir, agent count, status
- Session page: same pixi.js crab UI as before, self-contained
- Each session page connects to `ws://host:9091/ws/:session-id`

## Integration with `grotto spawn`
1. `grotto spawn` generates a semantic ID
2. Stores it in `.grotto/config.toml` as `session_id = "crimson-coral-tide"`
3. If daemon is running, POST to `/api/sessions` to register
4. If daemon is NOT running, auto-start it first
5. Print the URL: `🪸 Portal: http://192.168.86.247:9091/crimson-coral-tide`

## Integration with `grotto kill all`
1. If daemon is running, DELETE `/api/sessions/:id` to unregister
2. Session disappears from index page
3. Daemon keeps running for other sessions

## File Changes

### grotto-core changes:
- Add `session_id` field to `Config`
- Add word list module (`words.rs`) with `generate_session_id()` function
- ~50 adjectives: crimson, silent, bright, swift, golden, etc.
- ~50 nouns: coral, tide, reef, crab, wave, pearl, shell, etc.

### grotto-serve changes (rename conceptually to daemon):
- Add session registry (HashMap<String, SessionState>)
- File watchers per session (spawn/remove as sessions register/unregister)
- WS connections tagged by session ID
- REST API routes for session management
- State persistence: `~/.grotto/sessions.json`
- PID file management for daemon start/stop

### grotto-cli changes:
- Add `Daemon` subcommand (start/stop/status)
- `spawn` generates session ID, auto-registers with daemon
- `kill` unregisters from daemon
- Print portal URL after spawn

### web/ changes:
- `index.html` → session list page (no pixi.js, just links + status)
- `session.html` → per-session crab UI (renamed from current index.html)
- `app.js` → connects to `/ws/:id` instead of `/ws`
- Route handling: axum serves `session.html` for `/:id` routes

## Task Breakdown
- **Task 1**: Word list + session ID generation in grotto-core. Daemon commands (start/stop/status) in CLI. PID file + state file management.
- **Task 2**: Multi-session server — session registry, per-session file watchers, REST API, WS per session. Refactor grotto-serve.
- **Task 3**: Web UI — index page with session list, per-session route serving, update app.js for session-scoped WS. Integration testing.
