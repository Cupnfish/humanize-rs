use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct GateRepo {
    _tempdir: TempDir,
    root: PathBuf,
}

impl GateRepo {
    fn new(with_active_loop: bool) -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path().to_path_buf();

        run(Command::new("git").args(["init", "-q"]).current_dir(&root));
        run(Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&root));
        run(Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&root));
        fs::write(root.join("README.md"), "# temp\n").unwrap();
        run(Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&root));
        run(Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(&root));

        let loop_dir = root.join(".humanize/rlcr/2026-01-18_12-00-00");
        if with_active_loop {
            fs::create_dir_all(&loop_dir).unwrap();
            let branch = current_branch(&root);
            fs::write(
                loop_dir.join("state.md"),
                format!(
                    "---\ncurrent_round: 0\nmax_iterations: 42\ncodex_model: gpt-5.4\ncodex_effort: xhigh\ncodex_timeout: 5400\npush_every_round: false\nfull_review_round: 5\nplan_file: plan.md\nplan_tracked: false\nstart_branch: {}\nbase_branch: {}\nbase_commit: deadbeef\nreview_started: false\nask_codex_question: true\nsession_id:\nagent_teams: false\n---\n",
                    branch, branch
                ),
            )
            .unwrap();
            fs::write(root.join("plan.md"), "# Plan\n\nx\n").unwrap();
            fs::copy(root.join("plan.md"), loop_dir.join("plan.md")).unwrap();
        }

        Self {
            _tempdir: tempdir,
            root,
        }
    }
}

#[test]
fn gate_rlcr_allows_when_no_active_loop() {
    let repo = GateRepo::new(false);
    let output = Command::new(bin())
        .args([
            "gate",
            "rlcr",
            "--project-root",
            repo.root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("ALLOW"));
}

#[test]
fn gate_rlcr_blocks_and_maps_to_exit_10() {
    let repo = GateRepo::new(true);
    let output = Command::new(bin())
        .args([
            "gate",
            "rlcr",
            "--project-root",
            repo.root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(10));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("BLOCK:"));
    assert!(stdout.contains("BLOCK:"), "stdout={stdout}");
}

#[test]
fn gate_rlcr_json_mode_returns_raw_json() {
    let repo = GateRepo::new(true);
    let output = Command::new(bin())
        .args([
            "gate",
            "rlcr",
            "--project-root",
            repo.root.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(10));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"decision\":\"block\""));
}

fn current_branch(root: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn run(cmd: &mut Command) {
    let status = cmd.status().unwrap();
    assert!(status.success());
}
