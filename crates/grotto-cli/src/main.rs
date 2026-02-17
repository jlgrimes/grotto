use chrono::Utc;
use clap::{Parser, Subcommand};
use grotto_core::daemon::{self, SessionEntry, SessionRegistry};
use grotto_core::{Grotto, Result};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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
    /// Wait for all agents to finish, then print summary
    Wait {
        /// Poll interval in seconds
        #[arg(long, default_value = "5")]
        interval: u64,
    },
    /// Mark a task as complete
    Complete {
        /// Task ID to complete
        task_id: String,
    },
    /// Start the real-time WebSocket server + web UI
    Serve {
        /// Port to listen on
        #[arg(long, default_value = "9091")]
        port: u16,
        /// Don't auto-open browser
        #[arg(long)]
        no_open: bool,
    },
    /// Manage the grotto daemon (persistent multi-session server)
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Internal: run the daemon server process (used by `daemon start`)
    #[command(hide = true)]
    DaemonServe {
        /// Port to listen on
        #[arg(long, default_value = "9091")]
        port: u16,
        /// Path to web UI directory
        #[arg(long)]
        web_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon server (background)
    Start {
        /// Port to listen on
        #[arg(long, default_value = "9091")]
        port: u16,
    },
    /// Stop the running daemon
    Stop,
    /// Check daemon status
    Status,
}

fn main() {
    let cli = Cli::parse();

    let project_dir = cli
        .dir
        .unwrap_or_else(|| env::current_dir().expect("Failed to get current directory"));

    if let Err(e) = run_command(cli.command, project_dir) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run_command(command: Commands, project_dir: PathBuf) -> Result<()> {
    match command {
        Commands::Spawn { count, task } => spawn_agents(project_dir, count, task),
        Commands::View => view_session(),
        Commands::Status => show_status(project_dir),
        Commands::Steer { agent, message } => steer_agent(project_dir, agent, message),
        Commands::Broadcast { message } => broadcast_message(project_dir, message),
        Commands::Wait { interval } => wait_for_completion(project_dir, interval),
        Commands::Kill { target } => kill_target(project_dir, target),
        Commands::Log { agent } => show_log(project_dir, agent),
        Commands::Events { follow } => show_events(project_dir, follow),
        Commands::Claim { task_id, agent } => claim_task(project_dir, task_id, agent),
        Commands::Complete { task_id } => complete_task(project_dir, task_id),
        Commands::Serve { port, no_open } => serve(project_dir, port, no_open),
        Commands::Daemon { action } => run_daemon(project_dir, action),
        Commands::DaemonServe { port, web_dir } => daemon_serve(port, web_dir),
    }
}

fn spawn_agents(project_dir: PathBuf, count: usize, task: String) -> Result<()> {
    // Check dependencies before doing anything
    if let Err(missing) = Grotto::check_dependencies() {
        eprintln!("❌ Missing required dependencies: {}", missing.join(", "));
        for bin in &missing {
            match bin.as_str() {
                "tmux" => eprintln!(
                    "   Install tmux: sudo apt install tmux (Debian/Ubuntu) or brew install tmux (macOS)"
                ),
                "claude" => {
                    eprintln!("   Install Claude Code: npm install -g @anthropic-ai/claude-code")
                }
                _ => {}
            }
        }
        return Err(grotto_core::GrottoError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Missing dependencies: {}", missing.join(", ")),
        )));
    }

    // Kill any existing grotto session
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", "grotto"])
        .output();

    println!("🪸 Spawning {} agents for task: {}", count, task);

    // Initialize grotto project (generates session ID)
    let grotto = Grotto::new(&project_dir, count, task)?;
    let session_id = grotto.config.session_id.as_deref().unwrap_or("unknown");

    // Create new tmux session with first agent
    let first_agent_prompt = grotto.generate_claude_prompt("agent-1");
    let mut startup_output_chunks: Vec<String> = Vec::new();

    let output = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            "grotto",
            "-c",
            &project_dir.to_string_lossy(),
            "claude",
            "--dangerously-skip-permissions",
            "-p",
            &first_agent_prompt,
        ])
        .output()
        .expect("Failed to create tmux session");

    if !output.status.success() {
        eprintln!(
            "Failed to create tmux session: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(grotto_core::GrottoError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to create tmux session",
        )));
    }
    push_command_output("tmux new-session", &output, &mut startup_output_chunks);

    // Add additional agents as new panes
    for i in 2..=count {
        let agent_id = format!("agent-{}", i);
        let agent_prompt = grotto.generate_claude_prompt(&agent_id);

        let output = Command::new("tmux")
            .args([
                "split-window",
                "-t",
                "grotto",
                "-c",
                &project_dir.to_string_lossy(),
                "claude",
                "--dangerously-skip-permissions",
                "-p",
                &agent_prompt,
            ])
            .output()
            .expect("Failed to split window");

        if !output.status.success() {
            eprintln!(
                "Failed to create pane for {}: {}",
                agent_id,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        push_command_output(
            &format!("tmux split-window {}", agent_id),
            &output,
            &mut startup_output_chunks,
        );
    }

    // Tile the panes evenly
    let _ = Command::new("tmux")
        .args(["select-layout", "-t", "grotto", "tiled"])
        .output();

    for pane_index in 0..count {
        if let Some(captured) = capture_tmux_pane(&format!("grotto:0.{}", pane_index)) {
            startup_output_chunks.push(format!("pane {}:\n{}", pane_index, captured));
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(startup_check_delay_ms()));
    if !tmux_session_exists("grotto") {
        let startup_output = startup_output_chunks.join("\n");
        handle_startup_failure(&project_dir, &grotto, &startup_output)?;
        return Err(grotto_core::GrottoError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Agent startup failed: tmux session exited immediately",
        )));
    }

    println!("✅ Spawned {} agents in tmux session 'grotto'", count);
    println!("   Session: {}", session_id);
    println!("   Use 'grotto view' to attach and see all agents");
    println!("   Use 'grotto status' to see task board");

    // Register with daemon if running
    if daemon::is_daemon_running() {
        let mut registry = SessionRegistry::load();
        registry.register(SessionEntry {
            id: session_id.to_string(),
            dir: project_dir.display().to_string(),
            agent_count: count,
            task: grotto.config.task.clone(),
        });
        let _ = registry.save();
        let url = daemon::daemon_url(9091);
        println!("   🪸 Portal: {}/{}", url, session_id);
    }

    Ok(())
}

fn startup_check_delay_ms() -> u64 {
    env::var("GROTTO_STARTUP_CHECK_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1500)
}

fn tmux_session_exists(session_name: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn capture_tmux_pane(target: &str) -> Option<String> {
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", target, "-p"])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn push_command_output(label: &str, output: &std::process::Output, chunks: &mut Vec<String>) {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stdout.is_empty() {
        chunks.push(format!("{label} stdout:\n{stdout}"));
    }
    if !stderr.is_empty() {
        chunks.push(format!("{label} stderr:\n{stderr}"));
    }
}

fn handle_startup_failure(
    project_dir: &PathBuf,
    grotto: &Grotto,
    startup_output: &str,
) -> Result<()> {
    let output_lower = startup_output.to_lowercase();
    let rate_limit_detected = output_lower.contains("rate limit")
        || output_lower.contains("limit exceeded")
        || output_lower.contains("too many requests")
        || output_lower.contains("quota");

    let hint = if rate_limit_detected {
        Some("Detected possible rate limit. Check Claude usage limits and retry.")
    } else {
        None
    };

    let mut failed_grotto = Grotto::load(project_dir)?;
    for agent in failed_grotto.agents.values_mut() {
        agent.state = "failed".to_string();
        agent.current_task = None;
        agent.progress = if let Some(hint_msg) = hint {
            format!("startup_failed: {}", hint_msg)
        } else {
            "startup_failed".to_string()
        };
        agent.last_update = Utc::now();
    }
    let agent_ids: Vec<String> = failed_grotto.agents.keys().cloned().collect();
    for agent_id in agent_ids {
        failed_grotto.write_agent_status(&agent_id)?;
    }

    failed_grotto.log_event(
        "startup_failed",
        None,
        None,
        Some("Agent startup failed"),
        serde_json::json!({
            "reason": "startup_failed",
            "startup_output": startup_output,
            "hint": hint,
        }),
    )?;

    if let Some(session_id) = &grotto.config.session_id {
        let mut registry = SessionRegistry::load();
        if registry.unregister(session_id).is_some() {
            let _ = registry.save();
        }
    }

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
    let session_exists = tmux_session_exists("grotto");
    let has_startup_failed_agents = grotto.agents.values().any(|a| {
        a.state == "failed" || a.state == "error" || a.progress.contains("startup_failed")
    });

    if !session_exists && has_startup_failed_agents {
        println!("📺 Tmux session: grotto (startup failed)");
    } else if session_exists {
        println!("📺 Tmux session: grotto (active)");
    } else {
        println!("📺 Tmux session: grotto (not found)");
    }

    println!("\n🤖 Agents ({}):", grotto.agents.len());
    for (agent_id, agent) in &grotto.agents {
        let is_failed = agent.state == "failed"
            || agent.state == "error"
            || agent.progress.contains("startup_failed");
        let display_state = if is_failed {
            "failed"
        } else {
            agent.state.as_str()
        };
        let status_emoji = match display_state {
            "working" => "🔄",
            "idle" => "💤",
            "spawning" => "🚀",
            "failed" => "❌",
            _ => "❓",
        };

        println!(
            "  {} {} (pane {}) - {} - {}",
            status_emoji, agent_id, agent.pane_index, display_state, agent.progress
        );

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

    let agent_state = grotto
        .agents
        .get(&agent)
        .ok_or_else(|| grotto_core::GrottoError::AgentNotFound(agent.clone()))?;

    let pane_target = format!("grotto:0.{}", agent_state.pane_index);

    println!(
        "💬 Sending message to {} (pane {})...",
        agent, agent_state.pane_index
    );

    let output = Command::new("tmux")
        .args(["send-keys", "-t", &pane_target, &message, "Enter"])
        .output()
        .expect("Failed to send keys to tmux");

    if !output.status.success() {
        eprintln!(
            "Failed to send message: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        println!("✅ Message sent to {}", agent);

        // Log the steering event
        grotto.log_event(
            "agent_steered",
            Some(&agent),
            None,
            Some(&message),
            serde_json::json!({ "message": message }),
        )?;
    }

    Ok(())
}

fn broadcast_message(project_dir: PathBuf, message: String) -> Result<()> {
    let grotto = Grotto::load(&project_dir)?;

    println!(
        "📢 Broadcasting message to all {} agents...",
        grotto.agents.len()
    );

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
    grotto.log_event(
        "broadcast",
        None,
        None,
        Some(&message),
        serde_json::json!({ "message": message, "agent_count": grotto.agents.len() }),
    )?;

    Ok(())
}

fn kill_target(project_dir: PathBuf, target: String) -> Result<()> {
    if target == "all" {
        println!("💀 Killing entire grotto session...");

        // Unregister from daemon if running
        if daemon::is_daemon_running() {
            if let Ok(grotto) = Grotto::load(&project_dir) {
                if let Some(session_id) = &grotto.config.session_id {
                    let mut registry = SessionRegistry::load();
                    if registry.unregister(session_id).is_some() {
                        let _ = registry.save();
                        println!("   Unregistered session '{}' from daemon", session_id);
                    }
                }
            }
        }

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
    let agent_state = grotto
        .agents
        .get(&target)
        .ok_or_else(|| grotto_core::GrottoError::AgentNotFound(target.clone()))?;

    let pane_target = format!("grotto:0.{}", agent_state.pane_index);

    println!(
        "💀 Killing agent {} (pane {})...",
        target, agent_state.pane_index
    );

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
        grotto.log_event(
            "agent_killed",
            Some(&target),
            None,
            None,
            serde_json::json!({ "pane_index": agent_state.pane_index }),
        )?;
    } else {
        println!("❌ Failed to kill agent {}", target);
    }

    Ok(())
}

fn show_log(project_dir: PathBuf, agent: String) -> Result<()> {
    let grotto = Grotto::load(&project_dir)?;
    let agent_state = grotto
        .agents
        .get(&agent)
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
        println!(
            "❌ Failed to capture pane: {}",
            String::from_utf8_lossy(&output.stderr)
        );
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

fn wait_for_completion(project_dir: PathBuf, interval: u64) -> Result<()> {
    println!("⏳ Waiting for grotto agents to finish...");

    let start = std::time::Instant::now();

    loop {
        // Check if tmux session still exists
        let session_alive = Command::new("tmux")
            .args(["has-session", "-t", "grotto"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !session_alive {
            break;
        }

        let elapsed = start.elapsed().as_secs();
        let mins = elapsed / 60;
        let secs = elapsed % 60;
        eprint!("\r⏳ Agents working... ({mins}m {secs}s)   ");

        std::thread::sleep(std::time::Duration::from_secs(interval));
    }

    let elapsed = start.elapsed();
    let mins = elapsed.as_secs() / 60;
    let secs = elapsed.as_secs() % 60;

    eprintln!();
    println!("\n🪸 Grotto Complete ({mins}m {secs}s)");
    println!("{}", "=".repeat(50));

    // Load final state
    if let Ok(grotto) = Grotto::load(&project_dir) {
        println!("📋 Task: {}", grotto.config.task);
        println!("🤖 Agents: {}\n", grotto.config.agent_count);

        // Show task board
        let task_board_path = grotto.grotto_dir.join("tasks.md");
        if task_board_path.exists() {
            if let Ok(content) = fs::read_to_string(&task_board_path) {
                println!("{}", content);
            }
        }

        // Show event summary
        let events_path = grotto.grotto_dir.join("events.jsonl");
        if events_path.exists() {
            if let Ok(content) = fs::read_to_string(&events_path) {
                let event_count = content.lines().count();
                let claims: Vec<&str> = content
                    .lines()
                    .filter(|l| l.contains("task_claimed"))
                    .collect();
                let completions: Vec<&str> = content
                    .lines()
                    .filter(|l| l.contains("task_completed"))
                    .collect();

                println!(
                    "📡 Events: {} total ({} claims, {} completions)",
                    event_count,
                    claims.len(),
                    completions.len()
                );
            }
        }
    }

    // Write a summary file for the lead to consume
    let summary_path = project_dir.join(".grotto").join("summary.md");
    let summary = format!(
        "# Grotto Run Summary\n\n\
         - Duration: {mins}m {secs}s\n\
         - Status: All agents exited\n\
         - See `events.jsonl` for full event log\n\
         - See `tasks.md` for final task board\n"
    );
    let _ = fs::write(&summary_path, &summary);

    println!("\n✅ Summary written to .grotto/summary.md");

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

fn serve(project_dir: PathBuf, port: u16, no_open: bool) -> Result<()> {
    let grotto_dir = project_dir.join(".grotto");
    if !grotto_dir.exists() {
        eprintln!("No .grotto directory found. Run 'grotto spawn' first.");
        return Err(grotto_core::GrottoError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No .grotto directory found",
        )));
    }

    // Look for web/ directory relative to the project
    let web_dir = project_dir.join("web");
    let web_dir = if web_dir.exists() {
        Some(web_dir)
    } else {
        None
    };

    if !no_open {
        // Try to open browser after a short delay
        let url = format!("http://localhost:{}", port);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let _ = Command::new("xdg-open")
                .arg(&url)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .or_else(|_| {
                    Command::new("open")
                        .arg(&url)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                });
        });
    }

    let rt = tokio::runtime::Runtime::new().map_err(|e| {
        grotto_core::GrottoError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
    })?;

    rt.block_on(async {
        grotto_serve::run_server(grotto_dir, port, web_dir)
            .await
            .map_err(|e| grotto_core::GrottoError::Io(e))
    })
}

fn run_daemon(_project_dir: PathBuf, action: DaemonAction) -> Result<()> {
    match action {
        DaemonAction::Start { port } => daemon_start(port),
        DaemonAction::Stop => daemon_stop(),
        DaemonAction::Status => daemon_status(),
    }
}

fn daemon_start(port: u16) -> Result<()> {
    // Check if already running
    if daemon::is_daemon_running() {
        let pid = daemon::read_pid().unwrap_or(0);
        println!("Daemon already running (PID {})", pid);
        return Ok(());
    }

    // Find the grotto binary path (ourselves)
    let exe = env::current_exe().map_err(|e| {
        grotto_core::GrottoError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
    })?;

    // Ensure daemon state directory exists
    daemon::ensure_daemon_dir().map_err(grotto_core::GrottoError::Io)?;

    // Look for web/ directory — check a few reasonable places
    let web_dir = find_web_dir();

    println!("🪸 Starting grotto daemon on port {}...", port);

    // Build the command args for the daemon subprocess
    let mut args = vec![
        "daemon-serve".to_string(),
        "--port".to_string(),
        port.to_string(),
    ];
    if let Some(ref web) = web_dir {
        args.push("--web-dir".to_string());
        args.push(web.to_string_lossy().to_string());
    }

    // Fork a background process
    let child = Command::new(&exe)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(grotto_core::GrottoError::Io)?;

    let pid = child.id();
    daemon::write_pid(pid).map_err(grotto_core::GrottoError::Io)?;

    let url = daemon::daemon_url(port);
    println!("Daemon started (PID {})", pid);
    println!("   Portal: {}", url);

    Ok(())
}

fn daemon_stop() -> Result<()> {
    match daemon::read_pid() {
        Some(pid) => {
            println!("Stopping daemon (PID {})...", pid);

            // Send SIGTERM
            let output = Command::new("kill")
                .arg(pid.to_string())
                .output()
                .map_err(grotto_core::GrottoError::Io)?;

            if output.status.success() {
                daemon::remove_pid().map_err(grotto_core::GrottoError::Io)?;
                println!("Daemon stopped");
            } else {
                eprintln!("Failed to stop daemon (PID may be stale)");
                daemon::remove_pid().map_err(grotto_core::GrottoError::Io)?;
            }
        }
        None => {
            println!("No daemon running (no PID file found)");
        }
    }
    Ok(())
}

fn daemon_status() -> Result<()> {
    if daemon::is_daemon_running() {
        let pid = daemon::read_pid().unwrap_or(0);
        println!("Daemon: running (PID {})", pid);

        // Show registered sessions
        let registry = SessionRegistry::load();
        if registry.sessions.is_empty() {
            println!("Sessions: none");
        } else {
            println!("Sessions ({}):", registry.sessions.len());
            for (id, entry) in &registry.sessions {
                println!("  {} - {} ({} agents)", id, entry.dir, entry.agent_count);
            }
        }
    } else {
        println!("Daemon: not running");
        // Clean up stale PID file
        if daemon::read_pid().is_some() {
            let _ = daemon::remove_pid();
        }
    }
    Ok(())
}

fn daemon_serve(port: u16, web_dir: Option<PathBuf>) -> Result<()> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| {
        grotto_core::GrottoError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
    })?;

    rt.block_on(async {
        grotto_serve::run_daemon(port, web_dir)
            .await
            .map_err(|e| grotto_core::GrottoError::Io(e))
    })
}

/// Find the web/ directory, checking common locations.
fn find_web_dir() -> Option<PathBuf> {
    // 1. Relative to the binary
    if let Ok(exe) = env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            // Check if we're in target/debug or target/release
            let project_root = bin_dir.parent().and_then(|p| p.parent());
            if let Some(root) = project_root {
                let web = root.join("web");
                if web.exists() {
                    return Some(web);
                }
            }
        }
    }
    // 2. Current directory
    let cwd_web = PathBuf::from("web");
    if cwd_web.exists() {
        return Some(cwd_web.canonicalize().ok()?);
    }
    None
}
