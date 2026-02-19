use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Where daemon state lives: ~/.grotto/
fn daemon_home() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".grotto")
}

/// Path to the daemon PID file.
pub fn pid_file() -> PathBuf {
    daemon_home().join("daemon.pid")
}

/// Path to the sessions registry file.
pub fn sessions_file() -> PathBuf {
    daemon_home().join("sessions.json")
}

/// Ensure ~/.grotto/ directory exists.
pub fn ensure_daemon_dir() -> std::io::Result<()> {
    fs::create_dir_all(daemon_home())
}

/// A registered session in the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub id: String,
    pub dir: String,
    pub agent_count: usize,
    pub task: String,
}

/// The session registry persisted to ~/.grotto/sessions.json.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionRegistry {
    pub sessions: HashMap<String, SessionEntry>,
}

impl SessionRegistry {
    /// Load the registry from disk, returning an empty one if the file doesn't exist.
    pub fn load() -> Self {
        let path = sessions_file();
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save the registry to disk.
    pub fn save(&self) -> std::io::Result<()> {
        ensure_daemon_dir()?;
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        fs::write(sessions_file(), json)
    }

    /// Register a new session.
    pub fn register(&mut self, entry: SessionEntry) {
        self.sessions.insert(entry.id.clone(), entry);
    }

    /// Remove a session by ID.
    pub fn unregister(&mut self, id: &str) -> Option<SessionEntry> {
        self.sessions.remove(id)
    }
}

/// Write the daemon PID file.
pub fn write_pid(pid: u32) -> std::io::Result<()> {
    ensure_daemon_dir()?;
    fs::write(pid_file(), pid.to_string())
}

/// Read the daemon PID from the PID file, if it exists.
pub fn read_pid() -> Option<u32> {
    fs::read_to_string(pid_file())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Remove the PID file.
pub fn remove_pid() -> std::io::Result<()> {
    let path = pid_file();
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Check if the daemon process is actually running.
pub fn is_daemon_running() -> bool {
    match read_pid() {
        Some(pid) => {
            // Check if process exists
            Path::new(&format!("/proc/{}", pid)).exists()
                || std::process::Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
        }
        None => false,
    }
}

/// Get the daemon URL if it's running.
pub fn daemon_url(port: u16) -> String {
    // Try to get the LAN IP for external access
    if let Some(ip) = get_lan_ip() {
        format!("http://{}:{}", ip, port)
    } else {
        format!("http://localhost:{}", port)
    }
}

fn get_lan_ip() -> Option<String> {
    let output = std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.split_whitespace().next().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_registry_roundtrip() {
        let mut reg = SessionRegistry::default();
        reg.register(SessionEntry {
            id: "crimson-coral-tide".into(),
            dir: "/tmp/project".into(),
            agent_count: 3,
            task: "build stuff".into(),
        });

        let json = serde_json::to_string(&reg).unwrap();
        let loaded: SessionRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.sessions.len(), 1);
        assert!(loaded.sessions.contains_key("crimson-coral-tide"));
        assert_eq!(loaded.sessions["crimson-coral-tide"].agent_count, 3);
    }

    #[test]
    fn session_registry_unregister() {
        let mut reg = SessionRegistry::default();
        reg.register(SessionEntry {
            id: "test-one-two".into(),
            dir: "/tmp".into(),
            agent_count: 1,
            task: "test".into(),
        });
        assert_eq!(reg.sessions.len(), 1);
        let removed = reg.unregister("test-one-two");
        assert!(removed.is_some());
        assert_eq!(reg.sessions.len(), 0);
    }

    #[test]
    fn session_registry_empty() {
        let reg = SessionRegistry::default();
        assert!(reg.sessions.is_empty());
    }
}
