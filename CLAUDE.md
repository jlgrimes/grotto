# Grotto 🪸

Multi-agent orchestration CLI in Rust. Spawns and coordinates multiple Claude Code (or other coding agent) sessions working in parallel on a shared codebase.

## Architecture

- **CLI** (`grotto`) — spawns/monitors/steers agents via tmux
- **File-based IPC** — `.grotto/` directory in target project for task board + messaging
- **WebSocket event stream** — real-time events for UI consumers (claw-companion)
- No database, no server process — just files + tmux + an optional event port

## Crate Structure

```
grotto/
├── Cargo.toml          (workspace)
├── crates/
│   ├── grotto-core/    # Task board, messaging, agent state, event emitter
│   ├── grotto-cli/     # CLI binary
│   └── grotto-bridge/  # WebSocket server for UI consumers (future)
```

## .grotto/ Directory (created in target project)

```
.grotto/
├── config.toml         # Team config (agent count, roles, lead info)
├── tasks.md            # Shared task board (claimed/done/blocked)
├── events.jsonl        # Append-only event log (for UI replay)
├── messages/           # Inter-agent message files
│   ├── agent-1-to-agent-2.md
│   └── lead-to-agent-1.md
└── agents/
    ├── agent-1/
    │   ├── CLAUDE.md   # Agent's instructions + role
    │   └── status.json # { state: "working", task: "...", progress: "..." }
    └── agent-2/
        ├── CLAUDE.md
        └── status.json
```

## Event Stream (for UI)

Every state change appends to `.grotto/events.jsonl`:
```json
{"ts":1234,"type":"agent_spawned","agent":"agent-1","role":"backend"}
{"ts":1235,"type":"task_claimed","agent":"agent-1","task":"build auth API"}
{"ts":1236,"type":"message","from":"agent-1","to":"agent-2","text":"..."}
{"ts":1237,"type":"agent_done","agent":"agent-1"}
```

`grotto-bridge` (future) watches this file and streams via WebSocket to the Tauri companion app.

## CLI Commands

```
grotto spawn <n> "<task>" [--dir <path>]   # Spawn N agents
grotto status                                # Task board + agent states
grotto steer <agent> "<message>"            # Message one agent
grotto broadcast "<message>"                # Message all agents
grotto kill <agent|all>                      # Kill agent(s)
grotto log <agent>                           # Show agent output
grotto events [--follow]                     # Stream events (for piping to UI)
```

## Rules

- Keep grotto-core independent of any specific coding agent (claude, codex, etc.)
- Agent spawning is pluggable — tmux is the default backend
- Event log is the source of truth for UI — design it well
- Files are the IPC — no fancy protocols, agents already read/write files
- Single binary, installable via `cargo install grotto`
