use serde::{Deserialize, Serialize};
use std::fmt;
use std::process::Command;

/// Real-time phase of a Claude Code agent, inferred from tmux pane output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPhase {
    Starting,
    Thinking,
    Editing,
    Running,
    Idle,
    Finished,
    Error,
}

impl fmt::Display for AgentPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentPhase::Starting => write!(f, "starting"),
            AgentPhase::Thinking => write!(f, "thinking"),
            AgentPhase::Editing => write!(f, "editing"),
            AgentPhase::Running => write!(f, "running"),
            AgentPhase::Idle => write!(f, "idle"),
            AgentPhase::Finished => write!(f, "finished"),
            AgentPhase::Error => write!(f, "error"),
        }
    }
}

/// A snapshot of a single tmux pane's state at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub agent_id: String,
    pub pane_index: usize,
    /// Last ~50 lines of pane output
    pub raw_content: String,
    /// Inferred phase from the pane content
    pub phase: AgentPhase,
    /// Last non-empty line (useful for UI display)
    pub last_activity_line: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Capture raw text from a tmux pane.
///
/// Runs `tmux capture-pane -t {session}:0.{pane} -p -S -50` and returns the
/// output. Returns `None` if the pane doesn't exist or tmux isn't available.
pub fn capture_pane(session_name: &str, pane_index: usize) -> Option<String> {
    let target = format!("{}:0.{}", session_name, pane_index);
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", &target, "-p", "-S", "-50"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Infer the current agent phase from captured pane content.
///
/// Pattern-matches on Claude Code output conventions to determine what the
/// agent is currently doing.
pub fn infer_phase(content: &str) -> AgentPhase {
    if content.trim().is_empty() {
        return AgentPhase::Starting;
    }

    // Work with the last ~20 meaningful lines for recency-sensitive checks
    let lines: Vec<&str> = content.lines().collect();
    let recent: Vec<&str> = lines
        .iter()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(20)
        .copied()
        .collect();

    if recent.is_empty() {
        return AgentPhase::Starting;
    }

    let last_line = recent[0];
    // Join recent lines for broad pattern scanning
    let recent_text: String = recent.iter().copied().collect::<Vec<_>>().join("\n");

    // --- Finished detection ---
    // Pane capture returning only whitespace / very short content after agent ran
    if last_line.contains("/exit")
        || last_line.contains("exited")
        || recent_text.contains("Process exited")
        || recent_text.contains("session ended")
        || recent_text.contains("has been completed")
    {
        return AgentPhase::Finished;
    }

    // --- Error detection ---
    for pattern in &[
        "Error:",
        "error:",
        "rate limit",
        "Rate limit",
        "APIError",
        "API error",
        "panic",
        "PANIC",
        "fatal:",
        "FATAL",
        "overloaded",
    ] {
        // Only check recent lines for errors (not the entire history)
        if recent.iter().take(5).any(|l| l.contains(pattern)) {
            return AgentPhase::Error;
        }
    }

    // --- Thinking detection ---
    if last_line.contains("Thinking")
        || last_line.contains("thinking")
        || last_line.contains("⏳")
        || last_line.contains("◐")
        || last_line.contains("◓")
        || last_line.contains("◑")
        || last_line.contains("◒")
        || last_line.contains("⠋")
        || last_line.contains("⠙")
        || last_line.contains("⠹")
        || last_line.contains("⠸")
        || last_line.contains("⠼")
        || last_line.contains("⠴")
        || last_line.contains("⠦")
        || last_line.contains("⠧")
        || last_line.contains("⠇")
        || last_line.contains("⠏")
    {
        return AgentPhase::Thinking;
    }

    // --- Editing detection ---
    // Look for file operation indicators in recent lines
    let edit_patterns = [
        "Write(",
        "Edit(",
        "Created ",
        "Updated ",
        "Wrote ",
        "wrote ",
        "editing",
        "Creating ",
        "Modified ",
    ];
    for line in recent.iter().take(5) {
        for pat in &edit_patterns {
            if line.contains(pat) {
                return AgentPhase::Editing;
            }
        }
    }

    // --- Running detection ---
    // Command execution indicators
    let run_patterns = ["$ ", "Running", "running", "Bash(", "bash("];
    for line in recent.iter().take(5) {
        for pat in &run_patterns {
            if line.contains(pat) {
                return AgentPhase::Running;
            }
        }
    }

    // --- Idle detection ---
    // Prompt-like endings suggest the agent is waiting for input
    let trimmed = last_line.trim();
    if trimmed.ends_with('>')
        || trimmed.ends_with('$')
        || trimmed.ends_with('❯')
        || trimmed.ends_with('%')
        || trimmed.ends_with("claude>")
    {
        return AgentPhase::Idle;
    }

    // Default: still starting up
    AgentPhase::Starting
}

/// Capture snapshots for all agents in a tmux session.
pub fn capture_all_agents(session_name: &str, agent_count: usize) -> Vec<PaneSnapshot> {
    let mut snapshots = Vec::with_capacity(agent_count);
    let now = chrono::Utc::now();

    for i in 0..agent_count {
        let agent_id = format!("agent-{}", i + 1);
        let (raw_content, phase, last_activity_line) = match capture_pane(session_name, i) {
            Some(content) => {
                let phase = infer_phase(&content);
                let last_line = content
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
                    .to_string();
                (content, phase, last_line)
            }
            None => {
                // Pane doesn't exist — agent likely finished or session died
                (String::new(), AgentPhase::Finished, String::new())
            }
        };

        snapshots.push(PaneSnapshot {
            agent_id,
            pane_index: i,
            raw_content,
            phase,
            last_activity_line,
            timestamp: now,
        });
    }

    snapshots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_display() {
        assert_eq!(AgentPhase::Thinking.to_string(), "thinking");
        assert_eq!(AgentPhase::Editing.to_string(), "editing");
        assert_eq!(AgentPhase::Running.to_string(), "running");
        assert_eq!(AgentPhase::Finished.to_string(), "finished");
        assert_eq!(AgentPhase::Error.to_string(), "error");
        assert_eq!(AgentPhase::Idle.to_string(), "idle");
        assert_eq!(AgentPhase::Starting.to_string(), "starting");
    }

    #[test]
    fn phase_serialization_roundtrip() {
        let phase = AgentPhase::Thinking;
        let json = serde_json::to_string(&phase).unwrap();
        assert_eq!(json, "\"thinking\"");
        let back: AgentPhase = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AgentPhase::Thinking);
    }

    #[test]
    fn infer_empty_content() {
        assert_eq!(infer_phase(""), AgentPhase::Starting);
        assert_eq!(infer_phase("   \n  \n  "), AgentPhase::Starting);
    }

    #[test]
    fn infer_thinking() {
        assert_eq!(infer_phase("Thinking..."), AgentPhase::Thinking);
        assert_eq!(infer_phase("some output\n⏳ Processing"), AgentPhase::Thinking);
        // Spinner chars
        assert_eq!(infer_phase("Loading ⠋"), AgentPhase::Thinking);
    }

    #[test]
    fn infer_editing() {
        let content = "some text\nWrite(/home/user/project/src/main.rs)";
        assert_eq!(infer_phase(content), AgentPhase::Editing);

        let content = "some text\nEdit(/home/user/project/src/lib.rs)";
        assert_eq!(infer_phase(content), AgentPhase::Editing);

        let content = "some text\nCreated src/monitor.rs";
        assert_eq!(infer_phase(content), AgentPhase::Editing);
    }

    #[test]
    fn infer_running() {
        let content = "some text\n$ cargo build";
        assert_eq!(infer_phase(content), AgentPhase::Running);

        let content = "some text\nBash(cargo test)";
        assert_eq!(infer_phase(content), AgentPhase::Running);
    }

    #[test]
    fn infer_error() {
        let content = "working...\nError: connection refused";
        assert_eq!(infer_phase(content), AgentPhase::Error);

        let content = "working...\nrate limit exceeded";
        assert_eq!(infer_phase(content), AgentPhase::Error);

        let content = "working...\nAPIError: 500";
        assert_eq!(infer_phase(content), AgentPhase::Error);
    }

    #[test]
    fn infer_finished() {
        assert_eq!(infer_phase("done\n/exit"), AgentPhase::Finished);
        assert_eq!(infer_phase("Process exited with code 0"), AgentPhase::Finished);
    }

    #[test]
    fn infer_idle() {
        assert_eq!(infer_phase("ready\n$ "), AgentPhase::Running);
        assert_eq!(infer_phase("ready\nclaude>"), AgentPhase::Idle);
        assert_eq!(infer_phase("ready\n❯"), AgentPhase::Idle);
    }

    #[test]
    fn snapshot_serialization() {
        let snap = PaneSnapshot {
            agent_id: "agent-1".into(),
            pane_index: 0,
            raw_content: "hello world".into(),
            phase: AgentPhase::Thinking,
            last_activity_line: "hello world".into(),
            timestamp: chrono::Utc::now(),
        };

        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"phase\":\"thinking\""));
        let back: PaneSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.agent_id, "agent-1");
        assert_eq!(back.phase, AgentPhase::Thinking);
    }
}
