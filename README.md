# Grotto 🪸

Multi-agent orchestration CLI in Rust. Spawns and coordinates multiple Claude Code sessions working in parallel using tmux for process management.

## Architecture

**Simple and Direct**: No abstractions, no pluggable spawners, no complex traits. Just tmux + `claude` CLI.

- **One tmux session** called "grotto" with each agent as a separate pane
- **File-based coordination** via `.grotto/` directory for task board and messaging  
- **Direct CLI commands** for spawning, steering, and monitoring agents
- **Event logging** via JSON lines for debugging and UI integration

## Installation

```bash
cargo install --path crates/grotto-cli
```

## Quick Start

```bash
# Spawn 3 agents to work on a task
grotto spawn 3 "Build a web API with authentication"

# Attach to see all agents side-by-side
grotto view

# Check status and task board
grotto status

# Send message to specific agent
grotto steer agent-1 "Focus on the user registration endpoint first"

# Send message to all agents  
grotto broadcast "Code review: please check each other's work"

# View agent output
grotto log agent-2

# Kill specific agent or entire session
grotto kill agent-1
grotto kill all
```

## How It Works

### 1. Spawn Flow

When you run `grotto spawn N "task"`:

1. Creates `.grotto/` directory with config, task board, event log
2. `tmux new-session -d -s grotto` (creates the session)
3. First agent gets the initial pane  
4. Additional agents: `tmux split-window -t grotto` for each
5. `tmux select-layout -t grotto tiled` to arrange them evenly
6. Each pane runs `claude --dangerously-skip-permissions -p "..."` with task instructions

### 2. Agent Instructions

Each Claude Code session receives a prompt explaining:
- Its role as agent-N in pane N of the "grotto" session
- The main task and project directory
- Available `grotto` CLI commands for coordination
- Protocol for claiming tasks, communicating, and reporting progress

### 3. Coordination Commands

Agents can use these commands themselves:
- `grotto status` - See task board and peer states
- `grotto claim <task-id> --agent <agent-id>` - Claim a task
- `grotto complete <task-id>` - Mark task done  
- `grotto steer <other-agent> "message"` - Message a peer

### 4. Human Steering

The human (or lead agent) can:
- `grotto steer <agent> "message"` → `tmux send-keys -t grotto:0.N`
- `grotto broadcast "message"` → sends to all panes
- `grotto view` → `tmux attach -t grotto` to watch all agents
- `grotto log <agent>` → `tmux capture-pane -t grotto:0.N -p`

## .grotto/ Directory Structure

Created automatically in your project:

```
.grotto/
├── config.toml         # Team config (agent count, task description)  
├── tasks.md            # Shared task board (claimed/done/blocked)
├── events.jsonl        # Append-only event log
├── agents/
│   ├── agent-1/
│   │   └── status.json # Agent state and progress
│   └── agent-2/
│       └── status.json
└── messages/           # Future: inter-agent message files
```

## Commands Reference

### Core Commands

- `grotto spawn <N> "<task>"` - Spawn N agents in tmux session
- `grotto view` - Attach to tmux session to see all agents
- `grotto status` - Show task board and agent states  

### Communication

- `grotto steer <agent> "<message>"` - Send message to specific agent
- `grotto broadcast "<message>"` - Send message to all agents
- `grotto log <agent>` - Show agent's terminal output

### Task Management  

- `grotto claim <task-id> --agent <agent-id>` - Claim a task
- `grotto complete <task-id>` - Mark task as complete

### Process Management

- `grotto kill <agent>` - Kill specific agent (graceful then force)
- `grotto kill all` - Kill entire grotto session
- `grotto events [--follow]` - Show/follow event stream

## Examples

### Backend API Development
```bash
grotto spawn 3 "Build REST API with user auth, posts CRUD, and rate limiting"

# Agent specialization via steering
grotto steer agent-1 "You handle authentication and user management"  
grotto steer agent-2 "You handle the posts API and database schema"
grotto steer agent-3 "You handle rate limiting, middleware, and testing"
```

### Frontend Development  
```bash
grotto spawn 2 "Build React dashboard with charts and real-time updates"

grotto steer agent-1 "Focus on the UI components and styling"
grotto steer agent-2 "Handle WebSocket integration and state management"
```

### Bug Investigation
```bash  
grotto spawn 4 "Debug performance issue: API responses taking 2+ seconds"

grotto broadcast "Start by profiling different parts of the system"
grotto steer agent-1 "Profile the database queries"
grotto steer agent-2 "Check network and connection pooling"  
grotto steer agent-3 "Profile the application code"
grotto steer agent-4 "Monitor system resources during load"
```

## Implementation Notes

- **tmux session**: All agents in one session called "grotto", tiled layout
- **pane targeting**: `grotto:0.N` where N is the pane index (0-based)
- **graceful shutdown**: `/exit` sent to Claude Code, then `tmux kill-pane`
- **cross-platform**: Unix/Linux focused (tmux dependency)

## No Abstractions

This refactor removed all pluggable spawner abstractions. It's just:
- Rust CLI that calls tmux commands directly  
- File-based state management
- Direct process communication via tmux send-keys
- Simple, debuggable, no magic

The goal: **maximum simplicity** for **maximum reliability**.