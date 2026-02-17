# Grotto 🪸 - Refactored Architecture

Multi-agent orchestration CLI in Rust. **Simplified to use ONLY tmux + Claude Code**. No OpenClaw, no pluggable spawners, no abstractions.

## Architecture: Simple and Direct

- **One tmux session** called "grotto" with each agent as a separate pane
- **Direct tmux commands** for process management (no spawner trait)
- **File-based IPC** via `.grotto/` directory for coordination
- **Claude Code sessions** as agents, each with task awareness

## Key Changes from Original Design

**REMOVED:**
- ❌ Pluggable spawner trait/abstraction
- ❌ OpenClaw integration  
- ❌ WebSocket bridge (grotto-bridge crate)
- ❌ Complex agent backends

**KEPT:**
- ✅ File-based coordination (`.grotto/` directory)
- ✅ Task board and event logging
- ✅ CLI commands for steering and monitoring
- ✅ Multi-pane tmux workflow

## Crate Structure (Simplified)

```
grotto/
├── Cargo.toml          # Workspace
├── crates/
│   ├── grotto-core/    # Task board, state, events - file-based only
│   └── grotto-cli/     # CLI binary - tmux commands only
```

**Removed:** `grotto-bridge` crate (WebSocket server)

## Spawn Flow

```bash
grotto spawn 3 "Build a web API"
```

1. **Create project state**: `.grotto/` dir, config.toml, tasks.md, agents/
2. **Spawn tmux session**: `tmux new-session -d -s grotto`
3. **First agent**: Gets the initial pane, runs `claude --dangerously-skip-permissions -p "..."`
4. **Additional agents**: `tmux split-window -t grotto` for each
5. **Tile layout**: `tmux select-layout -t grotto tiled`
6. **Agent prompts**: Each gets task context + grotto CLI usage instructions

## Control Commands

### Human → Agents
- `grotto steer agent-1 "message"` → `tmux send-keys -t grotto:0.0`
- `grotto broadcast "message"` → send-keys to all panes
- `grotto view` → `tmux attach -t grotto`
- `grotto log agent-1` → `tmux capture-pane -t grotto:0.0 -p`

### Agents → System  
Agents can use grotto CLI themselves:
- `grotto status` - See task board
- `grotto claim main --agent agent-1` - Claim work
- `grotto complete main` - Mark done
- `grotto steer agent-2 "Can you review my code?"` - Peer communication

### Process Management
- `grotto kill agent-1` → `/exit` then `tmux kill-pane -t grotto:0.0`
- `grotto kill all` → `tmux kill-session -t grotto`

## .grotto/ Directory (Unchanged)

```
.grotto/
├── config.toml         # Agent count, task description
├── tasks.md            # Task board markdown
├── events.jsonl        # Event log (append-only)
├── agents/
│   ├── agent-1/status.json
│   └── agent-2/status.json  
└── messages/           # Future: message files
```

## Agent Prompt Template

Each Claude Code session gets:

```
You are agent-N, an autonomous coding agent in pane N of tmux session "grotto".

MAIN TASK: {task}
PROJECT: {project_dir}

You can coordinate with other agents using:
- `grotto status` - see task board
- `grotto claim <task> --agent agent-N` - claim work  
- `grotto complete <task>` - mark done
- `grotto steer <other-agent> "message"` - communicate

Start by checking `grotto status`, then claim available work.
```

## Implementation: Pure Rust + tmux

```rust
// No traits, no abstraction - just direct tmux calls
fn spawn_agents(count: usize, task: String) {
    Command::new("tmux")
        .args(["new-session", "-d", "-s", "grotto", "claude", "-p", &prompt])
        .output();
        
    for i in 2..=count {
        Command::new("tmux")
            .args(["split-window", "-t", "grotto", "claude", "-p", &prompt])
            .output();
    }
    
    Command::new("tmux")
        .args(["select-layout", "-t", "grotto", "tiled"])
        .output();
}

fn steer_agent(agent: &str, message: &str) {
    let pane = format!("grotto:0.{}", get_pane_index(agent));
    Command::new("tmux")
        .args(["send-keys", "-t", &pane, message, "Enter"])
        .output();
}
```

## Why This Approach?

**Simplicity**: No abstractions to debug, no traits to implement
**Reliability**: Direct tmux calls, well-understood process model  
**Visibility**: `grotto view` shows all agents working simultaneously
**Control**: Direct steering via tmux send-keys, immediate feedback
**Debugging**: `grotto log agent-N` captures exact terminal state

## Rules

1. **Keep grotto-core file-only**: No process management, just state + events
2. **Keep grotto-cli tmux-only**: No spawner plugins, just direct commands  
3. **One session model**: All agents in "grotto" session, tiled panes
4. **Claude Code agents**: Each pane runs `claude` with task context
5. **File coordination**: Agents use grotto CLI for task claiming/completion

This is the **maximally simple** multi-agent system. No magic, just files + tmux + Claude Code.