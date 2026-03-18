use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct TestRepo {
    tempdir: TempDir,
    loop_dir: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();

        run(Command::new("git").args(["init", "-q"]).current_dir(root));
        run(Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(root));
        run(Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(root));

        fs::write(root.join("init.txt"), "init\n").unwrap();
        run(Command::new("git")
            .args(["add", "init.txt"])
            .current_dir(root));
        run(Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(root));

        fs::write(root.join(".gitignore"), "plans/\n.humanize/\n").unwrap();
        run(Command::new("git")
            .args(["add", ".gitignore"])
            .current_dir(root));
        run(Command::new("git")
            .args(["commit", "-q", "-m", "ignore"])
            .current_dir(root));

        let loop_dir = root.join(".humanize/rlcr/2024-01-01_12-00-00");
        fs::create_dir_all(&loop_dir).unwrap();
        fs::create_dir_all(root.join("plans")).unwrap();

        Self { tempdir, loop_dir }
    }

    fn root(&self) -> &Path {
        self.tempdir.path()
    }

    fn branch(&self) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(self.root())
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn write_plan(&self, contents: &str) {
        fs::write(self.root().join("plans/test-plan.md"), contents).unwrap();
        fs::copy(
            self.root().join("plans/test-plan.md"),
            self.loop_dir.join("plan.md"),
        )
        .unwrap();
    }

    fn write_state(&self, review_started: bool, current_round: u32) {
        let branch = self.branch();
        fs::write(
            self.loop_dir.join("state.md"),
            format!(
                "---\ncurrent_round: {}\nmax_iterations: 10\ncodex_model: gpt-5.4\ncodex_effort: xhigh\ncodex_timeout: 5400\npush_every_round: false\nfull_review_round: 5\nplan_file: plans/test-plan.md\nplan_tracked: false\nstart_branch: {}\nbase_branch: {}\nbase_commit: deadbeef\nreview_started: {}\nask_codex_question: true\nsession_id:\nagent_teams: false\n---\n",
                current_round,
                branch,
                branch,
                if review_started { "true" } else { "false" }
            ),
        )
        .unwrap();
    }

    fn write_goal_tracker(&self) {
        fs::write(
            self.loop_dir.join("goal-tracker.md"),
            "# Goal Tracker\n## IMMUTABLE SECTION\n### Ultimate Goal\nDone\n### Acceptance Criteria\nDone\n---\n## MUTABLE SECTION\n#### Active Tasks\n| Task | Target AC | Status |\n|------|-----------|--------|\n| Test | AC-1 | completed |\n",
        )
        .unwrap();
    }

    fn mock_codex(&self, script: &str) -> PathBuf {
        let bin_dir = self.root().join(".humanize/test-bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let path = bin_dir.join("codex");
        fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
        }
        bin_dir
    }
}

#[test]
fn complete_transitions_to_finalize_phase() {
    let repo = TestRepo::new();
    repo.write_plan("# Plan\n## Goal\nDone\n## Requirements\n- one\n- two\n- three\n");
    repo.write_state(false, 3);
    repo.write_goal_tracker();
    fs::write(
        repo.loop_dir.join("round-3-summary.md"),
        "# Round 3 Summary\nImplemented all features.\n",
    )
    .unwrap();
    let bin_dir = repo.mock_codex(
        "#!/bin/bash\nif [[ \"$1\" == \"exec\" ]]; then\n  cat >/dev/null\n  printf 'All requirements met.\\n\\nCOMPLETE\\n'\nelif [[ \"$1\" == \"review\" ]]; then\n  printf 'No issues found.\\n'\nfi\n",
    );

    let output = Command::new(bin())
        .args(["stop", "rlcr"])
        .env("CLAUDE_PROJECT_DIR", repo.root())
        .env(
            "PATH",
            format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap()),
        )
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"decision\":\"block\""));
    assert!(repo.loop_dir.join("finalize-state.md").exists());
    assert!(!repo.loop_dir.join("state.md").exists());
    assert!(stdout.contains("Finalize Phase"));
}

#[test]
fn non_complete_feedback_increments_round_and_writes_review_result() {
    let repo = TestRepo::new();
    repo.write_plan("# Plan\n## Goal\nDone\n## Requirements\n- one\n- two\n- three\n");
    repo.write_state(false, 3);
    repo.write_goal_tracker();
    fs::write(
        repo.loop_dir.join("round-3-summary.md"),
        "# Round 3 Summary\nImplemented some features.\n",
    )
    .unwrap();
    fs::write(
        repo.root().join(".humanize/transcript.jsonl"),
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"TodoWrite\",\"input\":{\"todos\":[{\"content\":\"Task\",\"status\":\"completed\"}]}}]}}\n",
    )
    .unwrap();
    let bin_dir = repo.mock_codex(
        "#!/bin/bash\nif [[ \"$1\" == \"exec\" ]]; then\n  cat >/dev/null\n  printf '## Review Feedback\\n\\nSome issues need to be addressed:\\n- Issue 1: Fix the bug in function X\\n\\nCONTINUE\\n'\nfi\n",
    );

    let mut child = Command::new(bin())
        .args(["stop", "rlcr"])
        .env("CLAUDE_PROJECT_DIR", repo.root())
        .env(
            "PATH",
            format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap()),
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        write!(
            stdin,
            "{{\"transcript_path\":\"{}\"}}",
            repo.root().join(".humanize/transcript.jsonl").display()
        )
        .unwrap();
    }
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let state = fs::read_to_string(repo.loop_dir.join("state.md")).unwrap();
    assert!(state.contains("current_round: 4"), "state={state}");
    assert!(repo.loop_dir.join("round-3-review-result.md").exists());
    let prompt = fs::read_to_string(repo.loop_dir.join("round-4-prompt.md")).unwrap();
    assert!(prompt.contains("Issue 1"));
}

#[test]
fn finalize_phase_completes_without_codex() {
    let repo = TestRepo::new();
    fs::write(repo.root().join("plan.md"), "plan\n").unwrap();
    fs::write(
        repo.root().join(".gitignore"),
        "plans/\n.humanize/\nplan.md\n",
    )
    .unwrap();
    run(Command::new("git")
        .args(["add", ".gitignore"])
        .current_dir(repo.root()));
    run(Command::new("git")
        .args(["commit", "-q", "-m", "update ignore"])
        .current_dir(repo.root()));
    fs::copy(repo.root().join("plan.md"), repo.loop_dir.join("plan.md")).unwrap();
    let branch = repo.branch();
    fs::write(
        repo.loop_dir.join("finalize-state.md"),
        format!(
            "---\ncurrent_round: 5\nmax_iterations: 42\ncodex_model: gpt-5.4\ncodex_effort: xhigh\ncodex_timeout: 5400\npush_every_round: false\nfull_review_round: 5\nplan_file: plan.md\nplan_tracked: false\nstart_branch: {}\nbase_branch: {}\nbase_commit: deadbeef\nreview_started: true\nask_codex_question: true\nsession_id:\nagent_teams: false\n---\n",
            branch, branch
        ),
    )
    .unwrap();
    fs::write(
        repo.loop_dir.join("finalize-summary.md"),
        "# Finalize Summary\nDone.\n",
    )
    .unwrap();

    let output = Command::new(bin())
        .args(["stop", "rlcr"])
        .env("CLAUDE_PROJECT_DIR", repo.root())
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(repo.loop_dir.join("complete-state.md").exists());
    assert!(!repo.loop_dir.join("finalize-state.md").exists());
}

#[test]
fn implementation_phase_blocks_when_git_not_clean() {
    let repo = TestRepo::new();
    repo.write_plan("# Plan\n## Goal\nDone\n## Requirements\n- one\n- two\n- three\n");
    repo.write_state(false, 1);
    repo.write_goal_tracker();
    fs::write(
        repo.loop_dir.join("round-1-summary.md"),
        "# Round 1 Summary\nImplemented some features.\n",
    )
    .unwrap();
    fs::write(repo.root().join("dirty.txt"), "uncommitted\n").unwrap();

    let output = Command::new(bin())
        .args(["stop", "rlcr"])
        .env("CLAUDE_PROJECT_DIR", repo.root())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"decision\":\"block\""));
    assert!(stdout.contains("Git Not Clean"));
    assert!(repo.loop_dir.join("state.md").exists());
}

#[test]
fn full_alignment_round_writes_full_alignment_review_prompt() {
    let repo = TestRepo::new();
    repo.write_plan("# Plan\n## Goal\nDone\n## Requirements\n- one\n- two\n- three\n");
    repo.write_state(false, 4);
    repo.write_goal_tracker();
    fs::write(
        repo.loop_dir.join("round-4-summary.md"),
        "# Round 4 Summary\nImplemented some features.\n",
    )
    .unwrap();
    let bin_dir = repo.mock_codex(
        "#!/bin/bash\nif [[ \"$1\" == \"exec\" ]]; then\n  cat >/dev/null\n  printf '## Review Feedback\\n\\nContinue working.\\n\\nCONTINUE\\n'\nfi\n",
    );
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");

    let output = Command::new(bin())
        .args(["stop", "rlcr"])
        .env("CLAUDE_PROJECT_DIR", repo.root())
        .env("CLAUDE_PLUGIN_ROOT", plugin_root.display().to_string())
        .env(
            "PATH",
            format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap()),
        )
        .output()
        .unwrap();

    assert!(output.status.success());
    let review_prompt = fs::read_to_string(repo.loop_dir.join("round-4-review-prompt.md")).unwrap();
    assert!(review_prompt.contains("FULL GOAL ALIGNMENT CHECK"));
}

#[test]
fn resume_then_stop_still_runs_codex_review() {
    let repo = TestRepo::new();
    repo.write_plan("# Plan\n## Goal\nDone\n## Requirements\n- one\n- two\n- three\n");
    repo.write_state(false, 3);
    repo.write_goal_tracker();
    fs::write(
        repo.loop_dir.join("round-3-prompt.md"),
        "Resume RLCR prompt\n",
    )
    .unwrap();
    fs::write(
        repo.loop_dir.join("round-3-summary.md"),
        "# Round 3 Summary\nImplemented some features.\n",
    )
    .unwrap();
    fs::write(
        repo.root().join(".humanize/transcript.jsonl"),
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"TodoWrite\",\"input\":{\"todos\":[{\"content\":\"Task\",\"status\":\"completed\"}]}}]}}\n",
    )
    .unwrap();

    let marker = repo.root().join(".humanize/codex-called.log");
    let bin_dir = repo.mock_codex(&format!(
        "#!/bin/bash\nif [[ \"$1\" == \"exec\" ]]; then\n  echo CALLED >> \"{}\"\n  cat >/dev/null\n  printf '## Review Feedback\\n\\nSome issues need to be addressed:\\n- Issue 1\\n\\nCONTINUE\\n'\nfi\n",
        marker.display()
    ));

    let resume = Command::new(bin())
        .args(["resume", "rlcr"])
        .env("CLAUDE_PROJECT_DIR", repo.root())
        .output()
        .unwrap();
    assert!(
        resume.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&resume.stderr)
    );

    let mut child = Command::new(bin())
        .args(["stop", "rlcr"])
        .env("CLAUDE_PROJECT_DIR", repo.root())
        .env(
            "PATH",
            format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap()),
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        write!(
            stdin,
            "{{\"session_id\":\"new-session\",\"transcript_path\":\"{}\"}}",
            repo.root().join(".humanize/transcript.jsonl").display()
        )
        .unwrap();
    }
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"decision\":\"block\""), "stdout={stdout}");
    assert!(marker.exists(), "codex was not called after resume");
    let state = fs::read_to_string(repo.loop_dir.join("state.md")).unwrap();
    assert!(state.contains("current_round: 4"), "state={state}");
}

fn run(cmd: &mut Command) {
    let status = cmd.status().unwrap();
    assert!(status.success());
}
