use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use humanize_core::state::State;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct MonitorEnv {
    _tempdir: TempDir,
    project_dir: PathBuf,
}

impl MonitorEnv {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let project_dir = tempdir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();
        Self {
            _tempdir: tempdir,
            project_dir,
        }
    }

    fn project(&self) -> &Path {
        &self.project_dir
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::new(bin());
        cmd.env("CLAUDE_PROJECT_DIR", self.project().display().to_string());
        cmd
    }
}

#[test]
fn monitor_pr_once_shows_configured_and_active_bots() {
    let env = MonitorEnv::new();
    let loop_dir = env.project().join(".humanize/pr-loop/2026-01-18_12-00-00");
    fs::create_dir_all(&loop_dir).unwrap();
    State {
        current_round: 1,
        max_iterations: 42,
        start_branch: "feature/pr".to_string(),
        codex_model: "gpt-5.4".to_string(),
        codex_effort: "medium".to_string(),
        codex_timeout: 900,
        pr_number: Some(123),
        configured_bots: Some(vec!["claude".to_string(), "codex".to_string()]),
        active_bots: Some(vec!["claude".to_string()]),
        poll_interval: Some(30),
        poll_timeout: Some(900),
        started_at: Some("2026-01-18T10:00:00Z".to_string()),
        startup_case: Some("5".to_string()),
        latest_commit_sha: Some("abc123".to_string()),
        latest_commit_at: Some("2026-01-18T10:00:00Z".to_string()),
        ..State::default()
    }
    .save(loop_dir.join("state.md"))
    .unwrap();

    let output = env
        .cmd()
        .args(["monitor", "pr", "--once"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Configured Bots: claude, codex"));
    assert!(stdout.contains("Active Bots: claude"));
}

#[test]
fn monitor_pr_once_shows_none_for_empty_active_bots() {
    let env = MonitorEnv::new();
    let loop_dir = env.project().join(".humanize/pr-loop/2026-01-18_12-00-00");
    fs::create_dir_all(&loop_dir).unwrap();
    State {
        current_round: 1,
        max_iterations: 42,
        start_branch: "feature/pr".to_string(),
        codex_model: "gpt-5.4".to_string(),
        codex_effort: "medium".to_string(),
        codex_timeout: 900,
        pr_number: Some(123),
        configured_bots: Some(vec!["codex".to_string()]),
        active_bots: Some(vec![]),
        poll_interval: Some(30),
        poll_timeout: Some(900),
        started_at: Some("2026-01-18T10:00:00Z".to_string()),
        startup_case: Some("3".to_string()),
        latest_commit_sha: Some("abc123".to_string()),
        latest_commit_at: Some("2026-01-18T10:00:00Z".to_string()),
        ..State::default()
    }
    .save(loop_dir.join("state.md"))
    .unwrap();

    let output = env
        .cmd()
        .args(["monitor", "pr", "--once"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Active Bots: none"));
}

#[test]
fn monitor_skill_once_shows_latest_invocation_summary() {
    let env = MonitorEnv::new();
    let skill_dir = env
        .project()
        .join(".humanize/skill/2026-01-18_12-00-00-1234-aaaa");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("input.md"),
        "# Ask Codex Input\n\n## Question\n\nHow does this work?\n",
    )
    .unwrap();
    fs::write(skill_dir.join("output.md"), "It works.\n").unwrap();
    fs::write(
        skill_dir.join("metadata.md"),
        "---\nmodel: gpt-5.4\neffort: xhigh\nstatus: success\n---\n",
    )
    .unwrap();

    let output = env
        .cmd()
        .args(["monitor", "skill", "--once"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Total: 1"));
    assert!(stdout.contains("Status: success"));
    assert!(stdout.contains("gpt-5.4 (xhigh)"));
}
