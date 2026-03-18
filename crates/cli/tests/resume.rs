use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use humanize_core::state::State;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct ResumeEnv {
    _tempdir: TempDir,
    project_dir: PathBuf,
}

impl ResumeEnv {
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
fn resume_rlcr_replays_prompt_and_arms_session_handshake() {
    let env = ResumeEnv::new();
    let loop_dir = env.project().join(".humanize/rlcr/2026-01-18_12-00-00");
    fs::create_dir_all(&loop_dir).unwrap();

    let mut state = State::new_rlcr(
        "docs/plan.md".to_string(),
        false,
        "feature/resume".to_string(),
        "main".to_string(),
        "deadbeef".to_string(),
        Some(42),
        Some("gpt-5.4".to_string()),
        Some("xhigh".to_string()),
        Some(5400),
        false,
        Some(5),
        true,
        false,
        false,
    );
    state.session_id = Some("old-session".to_string());
    state.save(loop_dir.join("state.md")).unwrap();
    fs::write(loop_dir.join("round-0-prompt.md"), "Resume RLCR prompt\n").unwrap();

    let output = env.cmd().args(["resume", "rlcr"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== resume-rlcr-loop ==="));
    assert!(stdout.contains("Resume RLCR prompt"));
    assert!(stdout.contains("Session Rebind: armed"));

    let pending = fs::read_to_string(env.project().join(".humanize/.pending-session-id")).unwrap();
    assert!(pending.contains("humanize resume rlcr"));
    assert!(pending.contains(&loop_dir.join("state.md").display().to_string()));

    let updated_state = fs::read_to_string(loop_dir.join("state.md")).unwrap();
    assert!(updated_state.contains("session_id:"));
    assert!(!updated_state.contains("old-session"));
}

#[test]
fn resume_pr_replays_current_feedback_file() {
    let env = ResumeEnv::new();
    let loop_dir = env.project().join(".humanize/pr-loop/2026-01-18_12-00-00");
    fs::create_dir_all(&loop_dir).unwrap();

    State {
        current_round: 1,
        max_iterations: 42,
        codex_model: "gpt-5.4".to_string(),
        codex_effort: "medium".to_string(),
        codex_timeout: 900,
        pr_number: Some(123),
        configured_bots: Some(vec!["claude".to_string(), "codex".to_string()]),
        active_bots: Some(vec!["codex".to_string()]),
        ..State::default()
    }
    .save(loop_dir.join("state.md"))
    .unwrap();
    fs::write(
        loop_dir.join("round-1-pr-feedback.md"),
        "Resume PR feedback\n",
    )
    .unwrap();

    let output = env.cmd().args(["resume", "pr"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== resume-pr-loop ==="));
    assert!(stdout.contains("PR Number: #123"));
    assert!(stdout.contains("Configured Bots: claude, codex"));
    assert!(stdout.contains("Active Bots: codex"));
    assert!(stdout.contains("Resume PR feedback"));
}
