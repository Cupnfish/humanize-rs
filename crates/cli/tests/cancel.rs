use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct CancelEnv {
    _tempdir: TempDir,
    project_dir: PathBuf,
}

impl CancelEnv {
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
        cmd.current_dir(self.project());
        cmd.env("CLAUDE_PROJECT_DIR", self.project().display().to_string());
        cmd
    }

    fn create_rlcr_loop(&self, state_file_name: &str, state_contents: &str) -> PathBuf {
        let loop_dir = self.project().join(".humanize/rlcr/2026-03-19_00-00-00");
        fs::create_dir_all(&loop_dir).unwrap();
        fs::write(loop_dir.join(state_file_name), state_contents).unwrap();
        loop_dir
    }

    fn create_pr_loop(&self, state_contents: &str) -> PathBuf {
        let loop_dir = self.project().join(".humanize/pr-loop/2026-03-19_00-00-00");
        fs::create_dir_all(&loop_dir).unwrap();
        fs::write(loop_dir.join("state.md"), state_contents).unwrap();
        loop_dir
    }
}

#[test]
fn cancel_rlcr_without_active_loop_returns_no_loop_token() {
    let env = CancelEnv::new();

    let output = env.cmd().args(["cancel", "rlcr"]).output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("NO_LOOP"));
}

#[test]
fn cancel_rlcr_finalize_phase_requires_confirmation() {
    let env = CancelEnv::new();
    env.create_rlcr_loop(
        "finalize-state.md",
        "current_round: 7\nmax_iterations: 42\n",
    );

    let output = env.cmd().args(["cancel", "rlcr"]).output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("FINALIZE_NEEDS_CONFIRM"));
    assert!(stdout.contains("Use --force to cancel anyway."));
}

#[test]
fn cancel_pr_without_active_loop_returns_no_loop_token() {
    let env = CancelEnv::new();

    let output = env.cmd().args(["cancel", "pr"]).output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("NO_LOOP"));
}

#[test]
fn cancel_pr_active_loop_emits_cancelled_token() {
    let env = CancelEnv::new();
    let loop_dir = env.create_pr_loop("current_round: 3\nmax_iterations: 42\npr_number: 123\n");

    let output = env.cmd().args(["cancel", "pr"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("CANCELLED"));
    assert!(loop_dir.join("cancel-state.md").exists());
}
