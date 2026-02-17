use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::io::Write;
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
        
        let config = Config {
            agent_count,
            task: task.clone(),
            project_dir,
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
        grotto.log_event("team_spawned", None, None, Some("Team initialized"), serde_json::json!({
            "agent_count": agent_count,
            "task": &grotto.config.task
        }))?;
        
        Ok(grotto)
    }
    
    pub fn load(project_dir: impl AsRef<Path>) -> Result<Self> {
        let project_dir = project_dir.as_ref().to_path_buf();
        let grotto_dir = project_dir.join(".grotto");
        
        if !grotto_dir.exists() {
            return Err(GrottoError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No .grotto directory found. Run 'grotto spawn' first."
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
                status_emoji,
                task.id,
                task.description
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
            let task = self.tasks.iter_mut()
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
        
        self.log_event("task_claimed", Some(agent_id), Some(task_id), 
            Some(&format!("Agent {} claimed task {}", agent_id, task_id)),
            serde_json::json!({
                "task_description": &task_description
            })
        )?;
        
        Ok(())
    }
    
    pub fn complete_task(&mut self, task_id: &str) -> Result<()> {
        // Find task and update it, storing info for later use
        let (task_description, claimed_by_agent) = {
            let task = self.tasks.iter_mut()
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
        
        self.log_event("task_completed", claimed_by_agent.as_deref(), Some(task_id),
            Some(&format!("Task {} completed", task_id)),
            serde_json::json!({
                "task_description": &task_description
            })
        )?;
        
        Ok(())
    }
    
    pub fn write_agent_status(&self, agent_id: &str) -> Result<()> {
        let agent = self.agents.get(agent_id)
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
    
    pub fn generate_claude_prompt(&self, agent_id: &str) -> String {
        let agent = self.agents.get(agent_id).unwrap();
        
        format!(r#"You are {agent_id}, an autonomous coding agent working as part of a team on this task:

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