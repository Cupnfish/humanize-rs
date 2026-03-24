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
fn setup_rlcr_with_active_loop_auto_resumes() {
    let env = ResumeEnv::new();
    let loop_dir = env.project().join(".humanize/rlcr/2026-01-18_12-00-00");
    fs::create_dir_all(&loop_dir).unwrap();

    let mut state = State::new_rlcr(
        "docs/plan.md".to_string(),
        false,
        humanize_core::state::PlanMode::Snapshot,
        "docs/plan.md".to_string(),
        true,
        false,
        "sha256".to_string(),
        ".humanize/rlcr/2026-01-18_12-00-00/plan.md".to_string(),
        None,
        None,
        None,
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

    let output = env.cmd().args(["setup", "rlcr", "dummy.md"]).output().unwrap();
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
    assert!(pending.contains("humanize setup rlcr"));

    let updated_state = fs::read_to_string(loop_dir.join("state.md")).unwrap();
    assert!(updated_state.contains("session_id:"));
    assert!(!updated_state.contains("old-session"));
}

#[test]
fn setup_rlcr_auto_resume_review_pending_does_not_replay_stale_prompt() {
    let env = ResumeEnv::new();
    let loop_dir = env.project().join(".humanize/rlcr/2026-01-18_12-00-00");
    fs::create_dir_all(&loop_dir).unwrap();

    let mut state = State::new_rlcr(
        "docs/plan.md".to_string(),
        false,
        humanize_core::state::PlanMode::Snapshot,
        "docs/plan.md".to_string(),
        true,
        false,
        "sha256".to_string(),
        ".humanize/rlcr/2026-01-18_12-00-00/plan.md".to_string(),
        None,
        None,
        None,
        "feature/review".to_string(),
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
    state.current_round = 3;
    state.review_started = true;
    state.save(loop_dir.join("state.md")).unwrap();
    fs::write(
        loop_dir.join("round-3-prompt.md"),
        "Stale implementation prompt\n",
    )
    .unwrap();
    fs::write(
        loop_dir.join("round-4-review-prompt.md"),
        "Pending review prompt\n",
    )
    .unwrap();

    let output = env.cmd().args(["setup", "rlcr", "dummy.md"]).output().unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Phase: review-pending"), "stdout={stdout}");
    assert!(stdout.contains("Review Prompt: "), "stdout={stdout}");
    assert!(
        !stdout.contains("Stale implementation prompt"),
        "stdout={stdout}"
    );
}

#[test]
fn setup_rlcr_auto_resume_summary_ready_surfaces_review_pending() {
    let env = ResumeEnv::new();
    let loop_dir = env.project().join(".humanize/rlcr/2026-01-18_12-00-00");
    fs::create_dir_all(&loop_dir).unwrap();

    let mut state = State::new_rlcr(
        "docs/plan.md".to_string(),
        false,
        humanize_core::state::PlanMode::Snapshot,
        "docs/plan.md".to_string(),
        true,
        false,
        "sha256".to_string(),
        ".humanize/rlcr/2026-01-18_12-00-00/plan.md".to_string(),
        None,
        None,
        None,
        "feature/summary-ready".to_string(),
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
    state.current_round = 2;
    state.save(loop_dir.join("state.md")).unwrap();
    fs::write(
        loop_dir.join("round-2-prompt.md"),
        "Implementation prompt\n",
    )
    .unwrap();
    fs::write(
        loop_dir.join("round-2-summary.md"),
        "# Round 2 Summary\nFinished the work.\n",
    )
    .unwrap();

    let output = env.cmd().args(["setup", "rlcr", "dummy.md"]).output().unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Phase: review-pending"), "stdout={stdout}");
    assert!(stdout.contains("round-2-summary.md"), "stdout={stdout}");
    assert!(!stdout.contains("Implementation prompt"), "stdout={stdout}");
}

#[test]
fn setup_rlcr_auto_resume_legacy_state_falls_back_to_read_only() {
    let env = ResumeEnv::new();
    let loop_dir = env.project().join(".humanize/rlcr/2026-01-18_12-00-00");
    fs::create_dir_all(&loop_dir).unwrap();

    let legacy_state = r#"---
current_round: 2
max_iterations: 42
codex_model: gpt-5.4
codex_effort: xhigh
codex_timeout: 5400
push_every_round: false
full_review_round: 5
plan_file: docs/plan.md
plan_tracked: false
start_branch: feature/legacy
ask_codex_question: true
session_id:
agent_teams: false
---
"#;
    fs::write(loop_dir.join("state.md"), legacy_state).unwrap();
    fs::write(loop_dir.join("round-2-prompt.md"), "Legacy RLCR prompt\n").unwrap();

    let output = env.cmd().args(["setup", "rlcr", "dummy.md"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== resume-rlcr-loop ==="));
    assert!(stdout.contains("State Schema: legacy"));
    assert!(stdout.contains("Session Rebind: skipped"));
    assert!(stdout.contains("Legacy RLCR prompt"));
    assert!(!env.project().join(".humanize/.pending-session-id").exists());

    let unchanged_state = fs::read_to_string(loop_dir.join("state.md")).unwrap();
    assert_eq!(unchanged_state, legacy_state);
}
