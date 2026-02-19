# Grotto 🪸

**Multi-agent orchestration with tmux + Claude Code.**

Spawn a team of [Claude Code](https://docs.anthropic.com/en/docs/claude-code) agents in tmux that work in parallel — coordinated by your [OpenClaw](https://github.com/openclaw/openclaw) agent as team lead.

## Install

```bash
git clone https://github.com/jlgrimes/grotto.git /tmp/grotto
cd /tmp/grotto && cargo install --path crates/grotto-cli
rm -rf /tmp/grotto
```

Or tell your OpenClaw agent:

> Read https://raw.githubusercontent.com/jlgrimes/grotto/master/skill/SKILL.md and install grotto as a skill.

## How it works

```
┌─────────────────────────────────────┐
│  Your OpenClaw Agent (Team Lead)    │
└──────┬──────────┬──────────┬────────┘
       │          │          │
  ┌────▼───┐ ┌───▼────┐ ┌───▼────┐
  │Agent 1 │ │Agent 2 │ │Agent 3 │  ← Claude Code in tmux
  └────────┘ └────────┘ └────────┘
       ↕          ↕          ↕
  ┌──────────────────────────────────┐
  │  .grotto/ (file-based IPC)       │
  │  ├─ config.toml                  │
  │  ├─ tasks.md                     │
  │  ├─ events.jsonl                 │
  │  └─ agents/*/status.json         │
  └──────────────────────────────────┘
       ↕
  ┌──────────────────────────────────┐
  │  Daemon (port 9091)              │
  │  WebSocket + Web UI with crabs   │
  └──────────────────────────────────┘
```

Your OpenClaw agent spawns Claude Code sessions as tmux panes. They self-organize via a shared task board, communicate with each other, and ship code in parallel.

## Quick Start (recommended, persistent)

```bash
cd /path/to/project

# Spawn 3 agents
grotto spawn 3 "Build a REST API with auth, posts CRUD, and tests"

# Start the persistent web daemon (recommended default)
grotto daemon start --port 9091

# Optional: check daemon health
# grotto daemon status

# Watch agents work in tmux
grotto view

# Check status
grotto status
```

> Use `grotto serve` only for local debugging. For normal use, always run `grotto daemon start` so the portal survives shell/process interruptions.

## Commands

### Agent Management
- `grotto spawn <N> "<task>"` — Spawn N agents in a tmux session
- `grotto view` — Attach to the tmux session
- `grotto status` — Show task board and agent states
- `grotto steer <agent> "<message>"` — Message a specific agent
- `grotto broadcast "<message>"` — Message all agents
- `grotto log <agent>` — View an agent's terminal output
- `grotto kill <agent|all>` — Kill an agent or the entire session
- `grotto wait` — Block until all agents finish, then print summary

### Task Coordination
- `grotto claim <task-id> --agent <agent-id>` — Claim a task
- `grotto complete <task-id>` — Mark a task as done
- `grotto events [--follow]` — View or follow the event stream

### Daemon (Multi-Session Server)
- `grotto daemon start [--port 9091]` — Start the background daemon
- `grotto daemon stop` — Stop the daemon
- `grotto daemon status` — Check daemon status and list sessions

### Single-Session Server (debug only)
- `grotto serve [--port 9091]` — Run server for one session (foreground, non-persistent)

## Web UI

The daemon serves a web UI on port 9091 with:
- **Index page** — List of active sessions with links
- **Session page** — Animated pixel art crabs (one per agent) + live event log
- **Real-time updates** via WebSocket

Each session gets a semantic ID (e.g., `crimson-coral-tide`) used as the URL route.

## Requirements

- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) CLI
- [tmux](https://github.com/tmux/tmux)
- Rust toolchain

## Crate Structure

```
grotto/
├── crates/
│   ├── grotto-core/    # Task board, state, events, daemon registry
│   ├── grotto-cli/     # CLI binary
│   └── grotto-serve/   # HTTP/WebSocket server + file watcher + embedded UI assets
└── web/                # Source web assets (synced into grotto-serve/web for embedding)
```

## License

MIT
