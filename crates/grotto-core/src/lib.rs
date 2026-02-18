pub mod daemon;
pub mod words;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GrottoError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("Agent not found: {0}")]
    AgentNotFound(String),
    #[error("Task not found: {0}")]
    TaskNotFound(String),
}

pub type Result<T> = std::result::Result<T, GrottoError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub agent_count: usize,
    pub task: String,
    pub project_dir: PathBuf,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub id: String,
    pub pane_index: usize,
    pub state: String,
    pub current_task: Option<String>,
    pub progress: String,
    pub last_update: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub claimed_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Open,
    Claimed,
    InProgress,
    Completed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub agent_id: Option<String>,
    pub task_id: Option<String>,
    pub message: Option<String>,
    pub data: serde_json::Value,
}

#[derive(Debug)]
pub struct Grotto {
    pub grotto_dir: PathBuf,
    pub config: Config,
    pub agents: HashMap<String, AgentState>,
    pub tasks: Vec<Task>,
}

impl Grotto {
    pub fn new(project_dir: impl AsRef<Path>, agent_count: usize, task: String) -> Result<Self> {
        let project_dir = project_dir.as_ref().to_path_buf();
        let grotto_dir = project_dir.join(".grotto");

        // Create .grotto directory structure
        fs::create_dir_all(&grotto_dir)?;
        fs::create_dir_all(grotto_dir.join("agents"))?;
        fs::create_dir_all(grotto_dir.join("messages"))?;

        let session_id = words::generate_session_id();
        let config = Config {
            agent_count,
            task: task.clone(),
            project_dir,
            session_id: Some(session_id),
        };

        // Write config
        let config_path = grotto_dir.join("config.toml");
        let config_toml = toml::to_string(&config).unwrap();
        fs::write(config_path, config_toml)?;

        // Initialize task board
        let main_task = Task {
            id: "main".to_string(),
            description: task,
            status: TaskStatus::Open,
            claimed_by: None,
            created_at: Utc::now(),
            completed_at: None,
        };

        let tasks = vec![main_task];

        // Create agents
        let mut agents = HashMap::new();
        for i in 0..agent_count {
            let agent_id = format!("agent-{}", i + 1);
            let agent = AgentState {
                id: agent_id.clone(),
                pane_index: i,
                state: "spawning".to_string(),
                current_task: None,
                progress: "Starting up...".to_string(),
                last_update: Utc::now(),
            };
            // Create agent directory and files
            let agent_dir = grotto_dir.join("agents").join(&agent_id);
            fs::create_dir_all(&agent_dir)?;

            let status_path = agent_dir.join("status.json");
            let status_json = serde_json::to_string_pretty(&agent)?;
            fs::write(status_path, status_json)?;

            agents.insert(agent_id.clone(), agent);
        }

        let grotto = Grotto {
            grotto_dir,
            config,
            agents,
            tasks,
        };

        // Write initial task board
        grotto.write_task_board()?;

        // Log spawn event
        grotto.log_event(
            "team_spawned",
            None,
            None,
            Some("Team initialized"),
            serde_json::json!({
                "agent_count": agent_count,
                "task": &grotto.config.task
            }),
        )?;

        Ok(grotto)
    }

    pub fn load(project_dir: impl AsRef<Path>) -> Result<Self> {
        let project_dir = project_dir.as_ref().to_path_buf();
        let grotto_dir = project_dir.join(".grotto");

        if !grotto_dir.exists() {
            return Err(GrottoError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No .grotto directory found. Run 'grotto spawn' first.",
            )));
        }

        // Load config
        let config_path = grotto_dir.join("config.toml");
        let config_str = fs::read_to_string(config_path)?;
        let config: Config = toml::from_str(&config_str)?;

        // Load agents
        let mut agents = HashMap::new();
        let agents_dir = grotto_dir.join("agents");
        if agents_dir.exists() {
            for entry in fs::read_dir(agents_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    let agent_id = entry.file_name().to_string_lossy().to_string();
                    let status_path = entry.path().join("status.json");
                    if status_path.exists() {
                        let status_str = fs::read_to_string(status_path)?;
                        let agent: AgentState = serde_json::from_str(&status_str)?;
                        agents.insert(agent_id, agent);
                    }
                }
            }
        }

        // Load tasks from task board
        let tasks = Vec::new(); // We'll parse this from tasks.md if needed

        Ok(Grotto {
            grotto_dir,
            config,
            agents,
            tasks,
        })
    }

    pub fn write_task_board(&self) -> Result<()> {
        let task_board_path = self.grotto_dir.join("tasks.md");
        let mut content = String::new();
        content.push_str("# Task Board\n\n");

        for task in &self.tasks {
            let status_emoji = match task.status {
                TaskStatus::Open => "⭕",
                TaskStatus::Claimed => "🟡",
                TaskStatus::InProgress => "🔄",
                TaskStatus::Completed => "✅",
                TaskStatus::Blocked => "🚫",
            };

            content.push_str(&format!(
                "{} **{}** - {}\n",
                status_emoji, task.id, task.description
            ));

            if let Some(agent) = &task.claimed_by {
                content.push_str(&format!("   - Claimed by: {}\n", agent));
            }
            content.push('\n');
        }

        fs::write(task_board_path, content)?;
        Ok(())
    }

    pub fn claim_task(&mut self, task_id: &str, agent_id: &str) -> Result<()> {
        // Check if agent exists
        if !self.agents.contains_key(agent_id) {
            return Err(GrottoError::AgentNotFound(agent_id.to_string()));
        }

        // Find task and update it, storing description for later use
        let task_description = {
            let task = self
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or_else(|| GrottoError::TaskNotFound(task_id.to_string()))?;

            task.status = TaskStatus::Claimed;
            task.claimed_by = Some(agent_id.to_string());
            task.description.clone()
        };

        // Update agent state
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.current_task = Some(task_id.to_string());
            agent.state = "working".to_string();
            agent.progress = format!("Working on task: {}", task_description);
            agent.last_update = Utc::now();
        }

        // Write agent status
        self.write_agent_status(agent_id)?;

        self.write_task_board()?;

        self.log_event(
            "task_claimed",
            Some(agent_id),
            Some(task_id),
            Some(&format!("Agent {} claimed task {}", agent_id, task_id)),
            serde_json::json!({
                "task_description": &task_description
            }),
        )?;

        Ok(())
    }

    pub fn complete_task(&mut self, task_id: &str) -> Result<()> {
        // Find task and update it, storing info for later use
        let (task_description, claimed_by_agent) = {
            let task = self
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or_else(|| GrottoError::TaskNotFound(task_id.to_string()))?;

            task.status = TaskStatus::Completed;
            task.completed_at = Some(Utc::now());
            (task.description.clone(), task.claimed_by.clone())
        };

        // Update agent state if claimed by someone
        if let Some(agent_id) = &claimed_by_agent {
            if let Some(agent) = self.agents.get_mut(agent_id) {
                agent.current_task = None;
                agent.state = "idle".to_string();
                agent.progress = "Task completed, ready for next task".to_string();
                agent.last_update = Utc::now();
            }

            self.write_agent_status(agent_id)?;
        }

        self.write_task_board()?;

        self.log_event(
            "task_completed",
            claimed_by_agent.as_deref(),
            Some(task_id),
            Some(&format!("Task {} completed", task_id)),
            serde_json::json!({
                "task_description": &task_description
            }),
        )?;

        Ok(())
    }

    pub fn write_agent_status(&self, agent_id: &str) -> Result<()> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| GrottoError::AgentNotFound(agent_id.to_string()))?;

        let agent_dir = self.grotto_dir.join("agents").join(agent_id);
        let status_path = agent_dir.join("status.json");
        let status_json = serde_json::to_string_pretty(agent)?;
        fs::write(status_path, status_json)?;

        Ok(())
    }

    pub fn log_event(
        &self,
        event_type: &str,
        agent_id: Option<&str>,
        task_id: Option<&str>,
        message: Option<&str>,
        data: serde_json::Value,
    ) -> Result<()> {
        let event = Event {
            timestamp: Utc::now(),
            event_type: event_type.to_string(),
            agent_id: agent_id.map(|s| s.to_string()),
            task_id: task_id.map(|s| s.to_string()),
            message: message.map(|s| s.to_string()),
            data,
        };

        let events_path = self.grotto_dir.join("events.jsonl");
        let event_line = serde_json::to_string(&event)? + "\n";

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(events_path)?;
        file.write_all(event_line.as_bytes())?;

        Ok(())
    }

    /// Check if required external dependencies are available
    pub fn check_dependencies() -> std::result::Result<(), Vec<String>> {
        let mut missing = Vec::new();

        for bin in &["tmux", "claude"] {
            if Command::new("which")
                .arg(bin)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| !s.success())
                .unwrap_or(true)
            {
                missing.push(bin.to_string());
            }
        }

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }

    /// Get the pane index for an agent, validating it exists
    pub fn get_agent_pane(&self, agent_id: &str) -> Result<usize> {
        self.agents
            .get(agent_id)
            .map(|a| a.pane_index)
            .ok_or_else(|| GrottoError::AgentNotFound(agent_id.to_string()))
    }

    pub fn generate_claude_prompt(&self, agent_id: &str) -> String {
        let agent = self.agents.get(agent_id).unwrap();

        format!(
            r#"You are {agent_id}, an autonomous coding agent working as part of a team on this task:

**MAIN TASK**: {task}

You are working in: {project_dir}

## Your Role
- You are agent {agent_id} (pane {pane_index}) in a tmux session called "grotto"
- Work collaboratively with other agents on the shared codebase
- Use the `grotto` CLI to coordinate with your team

## Available Commands
- `grotto status` - See task board and agent states
- `grotto claim <task-id> --agent {agent_id}` - Claim a task
- `grotto complete <task-id>` - Mark a task as done
- `grotto steer <other-agent> "message"` - Send message to another agent
- `grotto broadcast "message"` - Message all agents
- `grotto log <agent>` - View another agent's output

## Coordination Protocol
1. Check `grotto status` to see available tasks
2. Claim tasks with `grotto claim <task-id> --agent {agent_id}`
3. Work on your claimed task
4. Mark it done with `grotto complete <task-id>`
5. Communicate with teammates as needed

## Working Directory
You are in: {project_dir}
Task board and coordination files are in: {project_dir}/.grotto/

Start by checking `grotto status` to see the current state, then claim an available task and begin working.
"#,
            agent_id = agent_id,
            task = self.config.task,
            project_dir = self.config.project_dir.display(),
            pane_index = agent.pane_index,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        (tmp, dir)
    }

    // === Initialization ===

    #[test]
    fn new_creates_grotto_directory_structure() {
        let (_tmp, dir) = setup();
        let grotto = Grotto::new(&dir, 2, "test task".into()).unwrap();

        assert!(dir.join(".grotto").exists());
        assert!(dir.join(".grotto/agents").exists());
        assert!(dir.join(".grotto/messages").exists());
        assert!(dir.join(".grotto/config.toml").exists());
        assert!(dir.join(".grotto/tasks.md").exists());
        assert!(dir.join(".grotto/events.jsonl").exists());
        assert!(dir.join(".grotto/agents/agent-1/status.json").exists());
        assert!(dir.join(".grotto/agents/agent-2/status.json").exists());
        assert_eq!(grotto.agents.len(), 2);
        assert_eq!(grotto.tasks.len(), 1);
    }

    #[test]
    fn new_creates_correct_agent_count() {
        let (_tmp, dir) = setup();
        let grotto = Grotto::new(&dir, 5, "big task".into()).unwrap();
        assert_eq!(grotto.agents.len(), 5);
        for i in 1..=5 {
            let id = format!("agent-{}", i);
            assert!(grotto.agents.contains_key(&id));
            assert_eq!(grotto.agents[&id].pane_index, i - 1);
        }
    }

    #[test]
    fn new_with_zero_agents() {
        let (_tmp, dir) = setup();
        let grotto = Grotto::new(&dir, 0, "empty".into()).unwrap();
        assert_eq!(grotto.agents.len(), 0);
    }

    #[test]
    fn new_writes_config_toml() {
        let (_tmp, dir) = setup();
        Grotto::new(&dir, 2, "my task".into()).unwrap();

        let config_str = fs::read_to_string(dir.join(".grotto/config.toml")).unwrap();
        assert!(config_str.contains("my task"));
        assert!(config_str.contains("agent_count = 2"));
        assert!(config_str.contains("session_id"));
    }

    #[test]
    fn new_generates_session_id() {
        let (_tmp, dir) = setup();
        let grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        let sid = grotto.config.session_id.as_ref().unwrap();
        let parts: Vec<&str> = sid.split('-').collect();
        assert_eq!(
            parts.len(),
            3,
            "session_id should be adjective-noun-noun: {}",
            sid
        );

        // Verify it's persisted and loadable
        let loaded = Grotto::load(&dir).unwrap();
        assert_eq!(loaded.config.session_id, grotto.config.session_id);
    }

    #[test]
    fn new_writes_initial_event() {
        let (_tmp, dir) = setup();
        Grotto::new(&dir, 1, "test".into()).unwrap();

        let events = fs::read_to_string(dir.join(".grotto/events.jsonl")).unwrap();
        assert!(events.contains("team_spawned"));
    }

    // === Loading ===

    #[test]
    fn load_from_existing_grotto() {
        let (_tmp, dir) = setup();
        Grotto::new(&dir, 3, "reload test".into()).unwrap();

        let loaded = Grotto::load(&dir).unwrap();
        assert_eq!(loaded.config.agent_count, 3);
        assert_eq!(loaded.config.task, "reload test");
        assert_eq!(loaded.agents.len(), 3);
    }

    #[test]
    fn load_fails_without_grotto_dir() {
        let (_tmp, dir) = setup();
        let result = Grotto::load(&dir);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No .grotto directory found"), "got: {}", err);
    }

    #[test]
    fn load_fails_with_nonexistent_dir() {
        let result = Grotto::load("/tmp/grotto-does-not-exist-98765");
        assert!(result.is_err());
    }

    // === Task claiming ===

    #[test]
    fn claim_task_success() {
        let (_tmp, dir) = setup();
        let mut grotto = Grotto::new(&dir, 2, "test".into()).unwrap();

        grotto.claim_task("main", "agent-1").unwrap();

        let task = &grotto.tasks[0];
        assert!(matches!(task.status, TaskStatus::Claimed));
        assert_eq!(task.claimed_by, Some("agent-1".to_string()));

        let agent = &grotto.agents["agent-1"];
        assert_eq!(agent.state, "working");
        assert_eq!(agent.current_task, Some("main".to_string()));
    }

    #[test]
    fn claim_task_nonexistent_agent() {
        let (_tmp, dir) = setup();
        let mut grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        let result = grotto.claim_task("main", "agent-99");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Agent not found: agent-99"), "got: {}", err);
    }

    #[test]
    fn claim_task_nonexistent_task() {
        let (_tmp, dir) = setup();
        let mut grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        let result = grotto.claim_task("nonexistent", "agent-1");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Task not found: nonexistent"), "got: {}", err);
    }

    #[test]
    fn claim_task_updates_status_file() {
        let (_tmp, dir) = setup();
        let mut grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        grotto.claim_task("main", "agent-1").unwrap();

        let status_str =
            fs::read_to_string(dir.join(".grotto/agents/agent-1/status.json")).unwrap();
        let agent: AgentState = serde_json::from_str(&status_str).unwrap();
        assert_eq!(agent.state, "working");
        assert_eq!(agent.current_task, Some("main".to_string()));
    }

    #[test]
    fn claim_task_logs_event() {
        let (_tmp, dir) = setup();
        let mut grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        grotto.claim_task("main", "agent-1").unwrap();

        let events = fs::read_to_string(dir.join(".grotto/events.jsonl")).unwrap();
        assert!(events.contains("task_claimed"));
    }

    // === Task completion ===

    #[test]
    fn complete_task_success() {
        let (_tmp, dir) = setup();
        let mut grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        grotto.claim_task("main", "agent-1").unwrap();
        grotto.complete_task("main").unwrap();

        let task = &grotto.tasks[0];
        assert!(matches!(task.status, TaskStatus::Completed));
        assert!(task.completed_at.is_some());

        let agent = &grotto.agents["agent-1"];
        assert_eq!(agent.state, "idle");
        assert_eq!(agent.current_task, None);
    }

    #[test]
    fn complete_task_nonexistent() {
        let (_tmp, dir) = setup();
        let mut grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        let result = grotto.complete_task("nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Task not found"));
    }

    #[test]
    fn complete_unclaimed_task() {
        let (_tmp, dir) = setup();
        let mut grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        // Complete without claiming — should still work
        grotto.complete_task("main").unwrap();
        assert!(matches!(grotto.tasks[0].status, TaskStatus::Completed));
    }

    // === Agent status ===

    #[test]
    fn write_agent_status_nonexistent() {
        let (_tmp, dir) = setup();
        let grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        let result = grotto.write_agent_status("agent-99");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Agent not found"));
    }

    #[test]
    fn get_agent_pane_success() {
        let (_tmp, dir) = setup();
        let grotto = Grotto::new(&dir, 3, "test".into()).unwrap();

        assert_eq!(grotto.get_agent_pane("agent-1").unwrap(), 0);
        assert_eq!(grotto.get_agent_pane("agent-2").unwrap(), 1);
        assert_eq!(grotto.get_agent_pane("agent-3").unwrap(), 2);
    }

    #[test]
    fn get_agent_pane_nonexistent() {
        let (_tmp, dir) = setup();
        let grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        let result = grotto.get_agent_pane("agent-99");
        assert!(result.is_err());
    }

    // === Event logging ===

    #[test]
    fn log_event_appends() {
        let (_tmp, dir) = setup();
        let grotto = Grotto::new(&dir, 1, "test".into()).unwrap();

        grotto
            .log_event(
                "custom",
                Some("agent-1"),
                None,
                Some("hello"),
                serde_json::json!({}),
            )
            .unwrap();
        grotto
            .log_event(
                "custom2",
                None,
                Some("task-1"),
                None,
                serde_json::json!({"key": "val"}),
            )
            .unwrap();

        let events = fs::read_to_string(dir.join(".grotto/events.jsonl")).unwrap();
        let lines: Vec<&str> = events.lines().collect();
        // 1 from new() + 2 manual = 3
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("custom"));
        assert!(lines[2].contains("custom2"));
    }

    // === Task board ===

    #[test]
    fn task_board_reflects_status_changes() {
        let (_tmp, dir) = setup();
        let mut grotto = Grotto::new(&dir, 1, "build something".into()).unwrap();

        let board = fs::read_to_string(dir.join(".grotto/tasks.md")).unwrap();
        assert!(board.contains("⭕")); // Open

        grotto.claim_task("main", "agent-1").unwrap();
        let board = fs::read_to_string(dir.join(".grotto/tasks.md")).unwrap();
        assert!(board.contains("🟡")); // Claimed
        assert!(board.contains("agent-1"));

        grotto.complete_task("main").unwrap();
        let board = fs::read_to_string(dir.join(".grotto/tasks.md")).unwrap();
        assert!(board.contains("✅")); // Completed
    }

    // === Prompt generation ===

    #[test]
    fn generate_prompt_contains_task_and_agent_info() {
        let (_tmp, dir) = setup();
        let grotto = Grotto::new(&dir, 2, "build a web API".into()).unwrap();

        let prompt = grotto.generate_claude_prompt("agent-1");
        assert!(prompt.contains("agent-1"));
        assert!(prompt.contains("build a web API"));
        assert!(prompt.contains("pane 0"));
        assert!(prompt.contains("grotto status"));
        assert!(prompt.contains("grotto claim"));

        let prompt2 = grotto.generate_claude_prompt("agent-2");
        assert!(prompt2.contains("agent-2"));
        assert!(prompt2.contains("pane 1"));
    }

    // === Dependency checking ===

    #[test]
    fn check_dependencies_runs() {
        // Just verify it doesn't panic — actual result depends on environment
        let result = Grotto::check_dependencies();
        match result {
            Ok(()) => {} // tmux + claude both found
            Err(missing) => {
                assert!(!missing.is_empty());
                for bin in &missing {
                    assert!(bin == "tmux" || bin == "claude");
                }
            }
        }
    }

    // === Edge cases ===

    #[test]
    fn reinitialize_overwrites_existing() {
        let (_tmp, dir) = setup();
        Grotto::new(&dir, 1, "first".into()).unwrap();
        let grotto = Grotto::new(&dir, 3, "second".into()).unwrap();

        assert_eq!(grotto.agents.len(), 3);
        assert_eq!(grotto.config.task, "second");
    }

    #[test]
    fn agent_states_serialize_roundtrip() {
        let agent = AgentState {
            id: "agent-1".into(),
            pane_index: 0,
            state: "working".into(),
            current_task: Some("main".into()),
            progress: "doing stuff".into(),
            last_update: Utc::now(),
        };

        let json = serde_json::to_string(&agent).unwrap();
        let back: AgentState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "agent-1");
        assert_eq!(back.state, "working");
        assert_eq!(back.current_task, Some("main".into()));
    }
}
