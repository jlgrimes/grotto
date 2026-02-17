use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn write_executable(path: &Path, contents: &str) {
    let mut file = fs::File::create(path).unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn setup_fake_binaries() -> (TempDir, String) {
    let bin_dir = TempDir::new().unwrap();

    let tmux_script = r#"#!/usr/bin/env bash
set -euo pipefail
cmd="${1:-}"
shift || true
state_dir="${GROTTO_TEST_STATE_DIR:-/tmp}"
session_file="$state_dir/tmux-session-alive"

case "$cmd" in
  kill-session)
    rm -f "$session_file"
    exit 0
    ;;
  new-session)
    if [ "${TMUX_CREATE_SESSION:-1}" = "1" ]; then
      touch "$session_file"
    fi
    if [ -n "${TMUX_NEW_STDOUT:-}" ]; then
      echo "${TMUX_NEW_STDOUT}"
    fi
    if [ -n "${TMUX_NEW_STDERR:-}" ]; then
      echo "${TMUX_NEW_STDERR}" >&2
    fi
    exit "${TMUX_NEW_EXIT:-0}"
    ;;
  split-window)
    if [ -n "${TMUX_SPLIT_STDOUT:-}" ]; then
      echo "${TMUX_SPLIT_STDOUT}"
    fi
    if [ -n "${TMUX_SPLIT_STDERR:-}" ]; then
      echo "${TMUX_SPLIT_STDERR}" >&2
    fi
    exit 0
    ;;
  select-layout)
    exit 0
    ;;
  has-session)
    if [ "${TMUX_HAS_SESSION:-0}" = "1" ] && [ -f "$session_file" ]; then
      exit 0
    fi
    exit 1
    ;;
  capture-pane)
    if [ "${TMUX_CAPTURE_EXIT:-1}" = "0" ]; then
      echo "${TMUX_CAPTURE_OUTPUT:-}"
      exit 0
    fi
    exit 1
    ;;
  *)
    exit 0
    ;;
esac
"#;

    let claude_script = r#"#!/usr/bin/env bash
exit 0
"#;

    write_executable(&bin_dir.path().join("tmux"), tmux_script);
    write_executable(&bin_dir.path().join("claude"), claude_script);

    let base_path = std::env::var("PATH").unwrap_or_default();
    let full_path = format!("{}:{}", bin_dir.path().display(), base_path);
    (bin_dir, full_path)
}

fn run_grotto(args: &[&str], home: &Path, path: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_grotto"))
        .args(args)
        .env("PATH", path)
        .env("HOME", home)
        .env("GROTTO_TEST_STATE_DIR", home)
        .env("GROTTO_STARTUP_CHECK_MS", "0")
        .output()
        .unwrap()
}

#[test]
fn spawn_marks_agents_failed_and_logs_startup_failure() {
    let (_bin_dir, path) = setup_fake_binaries();
    let project = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_grotto"))
        .args([
            "--dir",
            &project.path().display().to_string(),
            "spawn",
            "2",
            "test task",
        ])
        .env("PATH", &path)
        .env("HOME", home.path())
        .env("GROTTO_TEST_STATE_DIR", home.path())
        .env("GROTTO_STARTUP_CHECK_MS", "0")
        .env("TMUX_HAS_SESSION", "0")
        .env("TMUX_NEW_STDERR", "Claude error: rate limit exceeded")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "spawn should fail when tmux session dies"
    );

    let status_1 =
        fs::read_to_string(project.path().join(".grotto/agents/agent-1/status.json")).unwrap();
    let status_2 =
        fs::read_to_string(project.path().join(".grotto/agents/agent-2/status.json")).unwrap();
    assert!(status_1.contains("\"state\": \"failed\""));
    assert!(status_2.contains("\"state\": \"failed\""));
    assert!(status_1.contains("startup_failed"));
    assert!(status_1.contains("rate limit"));

    let events = fs::read_to_string(project.path().join(".grotto/events.jsonl")).unwrap();
    assert!(events.contains("\"event_type\":\"startup_failed\""));
    assert!(events.contains("rate limit"));
}

#[test]
fn status_shows_startup_failed_when_tmux_missing() {
    let (_bin_dir, path) = setup_fake_binaries();
    let project = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let spawn_output = Command::new(env!("CARGO_BIN_EXE_grotto"))
        .args([
            "--dir",
            &project.path().display().to_string(),
            "spawn",
            "1",
            "test task",
        ])
        .env("PATH", &path)
        .env("HOME", home.path())
        .env("GROTTO_TEST_STATE_DIR", home.path())
        .env("GROTTO_STARTUP_CHECK_MS", "0")
        .env("TMUX_HAS_SESSION", "0")
        .output()
        .unwrap();
    assert!(!spawn_output.status.success());

    let status_output = run_grotto(
        &["--dir", &project.path().display().to_string(), "status"],
        home.path(),
        &path,
    );

    assert!(status_output.status.success());
    let stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(stdout.contains("startup failed"), "stdout: {}", stdout);
    assert!(stdout.contains("failed"), "stdout: {}", stdout);
}

#[test]
fn spawn_success_behavior_unchanged_when_session_is_alive() {
    let (_bin_dir, path) = setup_fake_binaries();
    let project = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_grotto"))
        .args([
            "--dir",
            &project.path().display().to_string(),
            "spawn",
            "2",
            "test task",
        ])
        .env("PATH", &path)
        .env("HOME", home.path())
        .env("GROTTO_TEST_STATE_DIR", home.path())
        .env("GROTTO_STARTUP_CHECK_MS", "0")
        .env("TMUX_HAS_SESSION", "1")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "spawn should succeed when session is alive"
    );

    let events = fs::read_to_string(project.path().join(".grotto/events.jsonl")).unwrap();
    assert!(events.contains("\"event_type\":\"team_spawned\""));
    assert!(!events.contains("\"event_type\":\"startup_failed\""));
}
