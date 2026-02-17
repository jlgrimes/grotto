use clap::{Parser, Subcommand};
use grotto_core::{Grotto, Result};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::env;
use std::fs;

#[derive(Parser)]
#[command(name = "grotto")]
#[command(about = "🪸 Multi-agent orchestration with tmux + Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    #[arg(long, short, global = true)]
    dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Spawn N agents in tmux session
    Spawn {
        /// Number of agents to spawn
        count: usize,
        /// Main task description
        task: String,
    },
    /// Attach to the grotto tmux session
    View,
    /// Show task board and agent status  
    Status,
    /// Send message to specific agent
    Steer {
        /// Agent ID (agent-1, agent-2, etc.)
        agent: String,
        /// Message to send
        message: String,
    },
    /// Send message to all agents
    Broadcast {
        /// Message to send to all agents
        message: String,
    },
    /// Kill agent(s) or entire session
    Kill {
        /// Agent ID or "all"
        target: String,
    },
    /// Show agent's log output
    Log {
        /// Agent ID
        agent: String,
    },
    /// Show/follow event stream
    Events {
        /// Follow the event log
        #[arg(long, short)]
        follow: bool,
    },
    /// Claim a task
    Claim {
        /// Task ID to claim
        task_id: String,
        /// Agent ID claiming the task
        #[arg(long)]
        agent: String,
    },
    /// Mark a task as complete
    Complete {
        /// Task ID to complete
        task_id: String,
    },
}

fn main() {
    let cli = Cli::parse();
    
    let project_dir = cli.dir
        .unwrap_or_else(|| env::current_dir().expect("Failed to get current directory"));
    
    if let Err(e) = run_command(cli.command, project_dir) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run_command(command: Commands, project_dir: PathBuf) -> Result<()> {
    match command {
        Commands::Spawn { count, task } => {
            spawn_agents(project_dir, count, task)
        },
        Commands::View => {
            view_session()
        },
        Commands::Status => {
            show_status(project_dir)
        },
        Commands::Steer { agent, message } => {
            steer_agent(project_dir, agent, message)
        },
        Commands::Broadcast { message } => {
            broadcast_message(project_dir, message)
        },
        Commands::Kill { target } => {
            kill_target(project_dir, target)
        },
        Commands::Log { agent } => {
            show_log(project_dir, agent)
        },
        Commands::Events { follow } => {
            show_events(project_dir, follow)
        },
        Commands::Claim { task_id, agent } => {
            claim_task(project_dir, task_id, agent)
        },
        Commands::Complete { task_id } => {
            complete_task(project_dir, task_id)
        },
    }
}

fn spawn_agents(project_dir: PathBuf, count: usize, task: String) -> Result<()> {
    // Kill any existing grotto session
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", "grotto"])
        .output();
    
    println!("🪸 Spawning {} agents for task: {}", count, task);
    
    // Initialize grotto project
    let grotto = Grotto::new(&project_dir, count, task)?;
    
    // Create new tmux session with first agent
    let first_agent_prompt = grotto.generate_claude_prompt("agent-1");
    
    let output = Command::new("tmux")
        .args([
            "new-session", "-d", "-s", "grotto", "-c", &project_dir.to_string_lossy(),
            "claude", "--dangerously-skip-permissions", "-p", &first_agent_prompt
        ])
        .output()
        .expect("Failed to create tmux session");
    
    if !output.status.success() {
        eprintln!("Failed to create tmux session: {}", String::from_utf8_lossy(&output.stderr));
        return Err(grotto_core::GrottoError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to create tmux session"
        )));
    }
    
    // Add additional agents as new panes
    for i in 2..=count {
        let agent_id = format!("agent-{}", i);
        let agent_prompt = grotto.generate_claude_prompt(&agent_id);
        
        let output = Command::new("tmux")
            .args([
                "split-window", "-t", "grotto", "-c", &project_dir.to_string_lossy(),
                "claude", "--dangerously-skip-permissions", "-p", &agent_prompt
            ])
            .output()
            .expect("Failed to split window");
            
        if !output.status.success() {
            eprintln!("Failed to create pane for {}: {}", agent_id, String::from_utf8_lossy(&output.stderr));
        }
    }
    
    // Tile the panes evenly
    let _ = Command::new("tmux")
        .args(["select-layout", "-t", "grotto", "tiled"])
        .output();
    
    println!("✅ Spawned {} agents in tmux session 'grotto'", count);
    println!("   Use 'grotto view' to attach and see all agents");
    println!("   Use 'grotto status' to see task board");
    
    Ok(())
}

fn view_session() -> Result<()> {
    let output = Command::new("tmux")
        .args(["has-session", "-t", "grotto"])
        .output()
        .expect("Failed to check tmux session");
    
    if !output.status.success() {
        eprintln!("No grotto session found. Run 'grotto spawn' first.");
        return Ok(());
    }
    
    // Replace current process with tmux attach
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = Command::new("tmux")
            .args(["attach-session", "-t", "grotto"])
            .exec();
        // If we get here, exec failed
        eprintln!("Failed to attach to tmux session: {}", error);
    }
    
    #[cfg(not(unix))]
    {
        // On non-unix systems, just run the command normally
        let status = Command::new("tmux")
            .args(["attach-session", "-t", "grotto"])
            .status()
            .expect("Failed to run tmux");
        if !status.success() {
            eprintln!("Failed to attach to tmux session");
        }
    }
    
    Ok(())
}

fn show_status(project_dir: PathBuf) -> Result<()> {
    let grotto = Grotto::load(&project_dir)?;
    
    println!("🪸 Grotto Status");
    println!("================");
    println!("Project: {}", project_dir.display());
    println!("Task: {}\n", grotto.config.task);
    
    // Check if tmux session exists
    let session_exists = Command::new("tmux")
        .args(["has-session", "-t", "grotto"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    
    if session_exists {
        println!("📺 Tmux session: grotto (active)");
    } else {
        println!("📺 Tmux session: grotto (not found)");
    }
    
    println!("\n🤖 Agents ({}):", grotto.agents.len());
    for (agent_id, agent) in &grotto.agents {
        let status_emoji = match agent.state.as_str() {
            "working" => "🔄",
            "idle" => "💤",
            "spawning" => "🚀",
            _ => "❓",
        };
        
        println!("  {} {} (pane {}) - {} - {}",
            status_emoji, agent_id, agent.pane_index, agent.state, agent.progress);
        
        if let Some(task) = &agent.current_task {
            println!("      Current task: {}", task);
        }
    }
    
    println!("\n📋 Task Board:");
    let task_board_path = grotto.grotto_dir.join("tasks.md");
    if task_board_path.exists() {
        let content = fs::read_to_string(task_board_path)?;
        println!("{}", content);
    } else {
        println!("  No task board found");
    }
    
    Ok(())
}

fn steer_agent(project_dir: PathBuf, agent: String, message: String) -> Result<()> {
    let grotto = Grotto::load(&project_dir)?;
    
    let agent_state = grotto.agents.get(&agent)
        .ok_or_else(|| grotto_core::GrottoError::AgentNotFound(agent.clone()))?;
    
    let pane_target = format!("grotto:0.{}", agent_state.pane_index);
    
    println!("💬 Sending message to {} (pane {})...", agent, agent_state.pane_index);
    
    let output = Command::new("tmux")
        .args(["send-keys", "-t", &pane_target, &message, "Enter"])
        .output()
        .expect("Failed to send keys to tmux");
    
    if !output.status.success() {
        eprintln!("Failed to send message: {}", String::from_utf8_lossy(&output.stderr));
    } else {
        println!("✅ Message sent to {}", agent);
        
        // Log the steering event
        grotto.log_event("agent_steered", Some(&agent), None, Some(&message), 
            serde_json::json!({ "message": message }))?;
    }
    
    Ok(())
}

fn broadcast_message(project_dir: PathBuf, message: String) -> Result<()> {
    let grotto = Grotto::load(&project_dir)?;
    
    println!("📢 Broadcasting message to all {} agents...", grotto.agents.len());
    
    for (agent_id, agent_state) in &grotto.agents {
        let pane_target = format!("grotto:0.{}", agent_state.pane_index);
        
        let output = Command::new("tmux")
            .args(["send-keys", "-t", &pane_target, &message, "Enter"])
            .output()
            .expect("Failed to send keys to tmux");
        
        if output.status.success() {
            println!("  ✅ Sent to {}", agent_id);
        } else {
            println!("  ❌ Failed to send to {}", agent_id);
        }
    }
    
    // Log the broadcast event
    grotto.log_event("broadcast", None, None, Some(&message),
        serde_json::json!({ "message": message, "agent_count": grotto.agents.len() }))?;
    
    Ok(())
}

fn kill_target(project_dir: PathBuf, target: String) -> Result<()> {
    if target == "all" {
        println!("💀 Killing entire grotto session...");
        
        let output = Command::new("tmux")
            .args(["kill-session", "-t", "grotto"])
            .output()
            .expect("Failed to kill tmux session");
        
        if output.status.success() {
            println!("✅ Grotto session killed");
        } else {
            println!("❌ Failed to kill session (may not exist)");
        }
        
        return Ok(());
    }
    
    // Kill specific agent
    let grotto = Grotto::load(&project_dir)?;
    let agent_state = grotto.agents.get(&target)
        .ok_or_else(|| grotto_core::GrottoError::AgentNotFound(target.clone()))?;
    
    let pane_target = format!("grotto:0.{}", agent_state.pane_index);
    
    println!("💀 Killing agent {} (pane {})...", target, agent_state.pane_index);
    
    // Try graceful exit first
    let _ = Command::new("tmux")
        .args(["send-keys", "-t", &pane_target, "/exit", "Enter"])
        .output();
    
    // Wait a moment then force kill the pane
    std::thread::sleep(std::time::Duration::from_secs(2));
    
    let output = Command::new("tmux")
        .args(["kill-pane", "-t", &pane_target])
        .output()
        .expect("Failed to kill tmux pane");
    
    if output.status.success() {
        println!("✅ Agent {} killed", target);
        
        // Log the kill event
        grotto.log_event("agent_killed", Some(&target), None, None, 
            serde_json::json!({ "pane_index": agent_state.pane_index }))?;
    } else {
        println!("❌ Failed to kill agent {}", target);
    }
    
    Ok(())
}

fn show_log(project_dir: PathBuf, agent: String) -> Result<()> {
    let grotto = Grotto::load(&project_dir)?;
    let agent_state = grotto.agents.get(&agent)
        .ok_or_else(|| grotto_core::GrottoError::AgentNotFound(agent.clone()))?;
    
    let pane_target = format!("grotto:0.{}", agent_state.pane_index);
    
    println!("📜 Log for {} (pane {}):", agent, agent_state.pane_index);
    println!("{}", "=".repeat(50));
    
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", &pane_target, "-p"])
        .output()
        .expect("Failed to capture tmux pane");
    
    if output.status.success() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    } else {
        println!("❌ Failed to capture pane: {}", String::from_utf8_lossy(&output.stderr));
    }
    
    Ok(())
}

fn show_events(project_dir: PathBuf, follow: bool) -> Result<()> {
    let grotto_dir = project_dir.join(".grotto");
    let events_path = grotto_dir.join("events.jsonl");
    
    if !events_path.exists() {
        println!("No events file found");
        return Ok(());
    }
    
    if follow {
        println!("📡 Following events (Ctrl+C to stop)...");
        let output = Command::new("tail")
            .args(["-f", &events_path.to_string_lossy()])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .expect("Failed to tail events file");
            
        if !output.success() {
            eprintln!("Failed to follow events file");
        }
    } else {
        println!("📡 Recent events:");
        let content = fs::read_to_string(events_path)?;
        print!("{}", content);
    }
    
    Ok(())
}

fn claim_task(project_dir: PathBuf, task_id: String, agent: String) -> Result<()> {
    let mut grotto = Grotto::load(&project_dir)?;
    
    grotto.claim_task(&task_id, &agent)?;
    
    println!("✅ Task '{}' claimed by {}", task_id, agent);
    
    Ok(())
}

fn complete_task(project_dir: PathBuf, task_id: String) -> Result<()> {
    let mut grotto = Grotto::load(&project_dir)?;
    
    grotto.complete_task(&task_id)?;
    
    println!("🎉 Task '{}' marked as complete", task_id);
    
    Ok(())
}