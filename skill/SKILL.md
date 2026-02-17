---
name: grotto
description: Multi-agent orchestration for OpenClaw. Spawn a team of Claude Code agents in tmux to work on tasks in parallel. You are the team lead — spawn agents, steer them, monitor progress, and review output. Use when a task can be parallelized across multiple coding agents.
requires:
  binary: [grotto, tmux, claude]
---

# Grotto — Multi-Agent Orchestration 🪸

You can spawn a team of Claude Code agents to work in parallel using tmux. You're the team lead.

## Installation

If `grotto` isn't installed yet:

```bash
cd /tmp && git clone https://github.com/jlgrimes/grotto.git && cd grotto && cargo install --path crates/grotto-cli && cd / && rm -rf /tmp/grotto
```

Requires: `tmux`, `claude` (Claude Code CLI), Rust toolchain.

## Spawning a Team

```bash
# Spawn 3 agents to work on a task in the current directory
grotto spawn 3 "Build a REST API with auth, posts CRUD, and tests"
```

This creates:
- A tmux session called `grotto` with tiled panes (one per agent)
- A `.grotto/` directory with task board, config, and event log
- Each agent gets a Claude Code session with task context

## Steering Agents

```bash
# Message a specific agent
grotto steer agent-1 "Focus on authentication first"

# Broadcast to all
grotto broadcast "Run tests before marking anything complete"

# Check task board and status
grotto status
```

## Monitoring

```bash
# Attach to tmux to watch all agents side-by-side
grotto view

# View a specific agent's terminal output
grotto log agent-2

# Follow the event stream
grotto events --follow
```

## Task Management

Agents coordinate via a shared task board. They can:

```bash
grotto claim <task-id> --agent <agent-id>   # Claim work
grotto complete <task-id>                     # Mark done
grotto steer <other-agent> "message"          # Message peers
```

## Waiting for Completion

```bash
# Block until all agents finish, then get a summary
grotto wait
```

This polls the tmux session and prints a summary when all agents exit — duration, task board status, event counts. Also writes `.grotto/summary.md`.

**Use this after spawning agents** so you get notified when they're done instead of manually checking.

## Killing Agents

```bash
grotto kill agent-1    # Kill specific agent
grotto kill all        # Kill entire session
```

## Best Practices

- **Spawn 2-4 agents** — more than that and coordination overhead increases
- **Give clear, parallelizable tasks** — "build auth + API + tests" works; "build one thing sequentially" doesn't
- **Steer early** — assign specializations right after spawn so agents don't duplicate work
- **Monitor with `grotto status`** — check the task board to see who's doing what
- **Kill when done** — always `grotto kill all` after the work is complete

## Architecture

```
You (OpenClaw agent / team lead)
  ├── grotto spawn → tmux session with N panes
  ├── grotto steer → tmux send-keys to specific pane
  ├── grotto log   → tmux capture-pane output
  └── grotto kill  → graceful shutdown then kill

.grotto/ (file-based coordination)
  ├── config.toml      # Team config
  ├── tasks.md         # Shared task board
  ├── events.jsonl     # Event log
  └── agents/          # Per-agent status
```

All state is file-based. All process management is tmux. No abstractions, no magic.
