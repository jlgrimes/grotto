# Grotto 🪸

**Multi-agent orchestration for [OpenClaw](https://github.com/openclaw/openclaw).** Spawn a team of Claude Code agents in tmux to work on tasks in parallel — coordinated by your OpenClaw agent as team lead.

## What is this?

OpenClaw gives your AI agent persistence, memory, and tools. Grotto gives it a **team**. Your OpenClaw agent (the "lead") can spawn multiple Claude Code sessions that work simultaneously on different parts of a problem, communicate with each other, and coordinate via a shared task board.

Think of it as your AI's ability to delegate work to junior agents and manage them like a tech lead.

## How it works

```
┌─────────────────────────────────────────────┐
│  OpenClaw Agent (Team Lead)                 │
│  - Decides what to build                    │
│  - Spawns grotto agents                     │
│  - Steers and monitors progress             │
│  - Reviews output                           │
└──────────┬──────────┬──────────┬────────────┘
           │          │          │
     ┌─────▼──┐ ┌─────▼──┐ ┌────▼───┐
     │Agent 1 │ │Agent 2 │ │Agent 3 │  ← Claude Code in tmux panes
     │(auth)  │ │(API)   │ │(tests) │
     └────────┘ └────────┘ └────────┘
           │          │          │
           └──────────┼──────────┘
                      │
              .grotto/ (shared state)
              ├── tasks.md
              ├── events.jsonl
              └── agents/
```

- **One tmux session** with tiled panes — one per agent
- **File-based coordination** via `.grotto/` directory (task board, events, messages)
- **Agents can talk to each other** using grotto CLI commands
- **Lead steers via tmux** send-keys — no fire-and-forget, fully interactive

## Why not OpenClaw sub-agents?

OpenClaw's built-in `sessions_spawn` is fire-and-forget: you send a task, get a result back. You can't have a conversation mid-task, and steering restarts the agent. Grotto agents are **interactive** — the lead (or human) can steer, redirect, and collaborate with them in real-time via tmux.

## Installation

```bash
cargo install --path crates/grotto-cli
```

## Usage

Your OpenClaw agent uses grotto as a tool. In practice, the lead agent runs these commands via `exec`:

```bash
# Spawn 3 agents to work on a task
grotto spawn 3 "Build a web API with authentication"

# Watch them work (attaches to tmux)
grotto view

# Check the task board
grotto status

# Steer a specific agent
grotto steer agent-1 "Focus on user registration first"

# Broadcast to all agents
grotto broadcast "Run tests before marking anything complete"

# View an agent's terminal output
grotto log agent-2

# Kill when done
grotto kill all
```

### Agent self-coordination

Agents aren't just workers — they coordinate with each other:

```bash
# An agent checks what needs doing
grotto status

# Claims a task
grotto claim auth --agent agent-1

# Marks it done
grotto complete auth

# Messages a peer
grotto steer agent-2 "I changed the User schema, update your imports"
```

## OpenClaw Skill

Install as an OpenClaw skill so your agent knows how to use grotto:

```bash
# Copy the skill to your OpenClaw workspace
cp -r skill/ ~/.openclaw/workspace/skills/grotto/
```

Or reference it in your agent's `TOOLS.md`.

## Commands

| Command | Description |
|---------|-------------|
| `grotto spawn <N> "<task>"` | Spawn N Claude Code agents in tmux |
| `grotto view` | Attach to tmux session |
| `grotto status` | Show task board and agent states |
| `grotto steer <agent> "<msg>"` | Send message to specific agent |
| `grotto broadcast "<msg>"` | Message all agents |
| `grotto log <agent>` | Show agent's terminal output |
| `grotto claim <task> --agent <id>` | Claim a task |
| `grotto complete <task>` | Mark task done |
| `grotto kill <agent>` | Kill specific agent |
| `grotto kill all` | Kill entire session |
| `grotto events [--follow]` | Show/follow event stream |

## Requirements

- [OpenClaw](https://github.com/openclaw/openclaw) (your AI agent runtime)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) CLI (`claude`)
- [tmux](https://github.com/tmux/tmux)
- Rust toolchain (to build)

## License

MIT
