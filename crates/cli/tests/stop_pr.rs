use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use humanize_core::state::State;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct PrStopEnv {
    _tempdir: TempDir,
    project_dir: PathBuf,
    bin_dir: PathBuf,
    fixtures_dir: PathBuf,
    loop_dir: PathBuf,
}

impl PrStopEnv {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let project_dir = root.join("project");
        let bin_dir = root.join("bin");
        let fixtures_dir = root.join("fixtures");
        let loop_dir = project_dir.join(".humanize/pr-loop/2026-01-18_12-00-00");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&fixtures_dir).unwrap();
        fs::create_dir_all(&loop_dir).unwrap();

        let mock_gh_src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/mocks/gh");
        fs::copy(&mock_gh_src, bin_dir.join("gh")).unwrap();
        make_executable(&bin_dir.join("gh"));

        write_fixture(&fixtures_dir, "issue-comments.json", "[]");
        write_fixture(&fixtures_dir, "review-comments.json", "[]");
        write_fixture(&fixtures_dir, "pr-reviews.json", "[]");
        write_fixture(&fixtures_dir, "reactions.json", "[]");
        write_fixture(&fixtures_dir, "comment-reactions.json", "[]");

        run(Command::new("git").args(["init", "-q"]).current_dir(&project_dir));
        run(
            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(&project_dir),
        );
        run(
            Command::new("git")
                .args(["config", "user.name", "Test"])
                .current_dir(&project_dir),
        );
        fs::write(project_dir.join("README.md"), "# temp\n").unwrap();
        run(Command::new("git").args(["add", "README.md"]).current_dir(&project_dir));
        run(
            Command::new("git")
                .args(["commit", "-q", "-m", "init"])
                .current_dir(&project_dir),
        );

        Self {
            _tempdir: tempdir,
            project_dir,
            bin_dir,
            fixtures_dir,
            loop_dir,
        }
    }

    fn path_env(&self) -> String {
        format!("{}:{}", self.bin_dir.display(), std::env::var("PATH").unwrap())
    }

    fn project(&self) -> &Path {
        &self.project_dir
    }

    fn fixtures(&self) -> &Path {
        &self.fixtures_dir
    }

    fn branch(&self) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&self.project_dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn head_sha(&self) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&self.project_dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn env_cmd(&self) -> Command {
        let mut cmd = Command::new(bin());
        let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        cmd.env("PATH", self.path_env())
            .env("MOCK_GH_FIXTURES_DIR", self.fixtures().display().to_string())
            .env("MOCK_GH_PR_NUMBER", "123")
            .env("MOCK_GH_PR_STATE", "OPEN")
            .env("MOCK_GH_LATEST_COMMIT_AT", "2026-01-18T10:00:00Z")
            .env("MOCK_GH_HEAD_SHA", self.head_sha())
            .env("CLAUDE_PLUGIN_ROOT", plugin_root.display().to_string())
            .env("CLAUDE_PROJECT_DIR", self.project().display().to_string());
        cmd
    }

    fn mock_codex(&self, script: &str) {
        let path = self.bin_dir.join("codex");
        fs::write(&path, script).unwrap();
        make_executable(&path);
    }

    fn write_state(&self, state: State) {
        state.save(self.loop_dir.join("state.md")).unwrap();
    }

    fn write_resolve(&self, round: u32) {
        fs::write(
            self.loop_dir.join(format!("round-{}-pr-resolve.md", round)),
            "# Resolution Summary\n\nDone.\n",
        )
        .unwrap();
    }
}

#[test]
fn stop_pr_empty_active_bots_approves_immediately() {
    let env = PrStopEnv::new();
    env.write_state(State {
        current_round: 1,
        max_iterations: 42,
        start_branch: env.branch(),
        codex_model: "gpt-5.4".to_string(),
        codex_effort: "medium".to_string(),
        codex_timeout: 900,
        pr_number: Some(123),
        configured_bots: Some(vec!["codex".to_string()]),
        active_bots: Some(vec![]),
        poll_interval: Some(1),
        poll_timeout: Some(2),
        started_at: Some("2026-01-18T10:00:00Z".to_string()),
        startup_case: Some("3".to_string()),
        latest_commit_sha: Some(env.head_sha()),
        latest_commit_at: Some("2026-01-18T10:00:00Z".to_string()),
        last_trigger_at: Some("2026-01-18T11:00:00Z".to_string()),
        trigger_comment_id: Some("123".to_string()),
        ..State::default()
    });
    env.write_resolve(1);

    let output = env.env_cmd().args(["stop", "pr"]).output().unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(env.loop_dir.join("approve-state.md").exists());
    assert!(!env.loop_dir.join("state.md").exists());
}

#[test]
fn stop_pr_case1_codex_thumbsup_approves_without_trigger() {
    let env = PrStopEnv::new();
    write_fixture(
        env.fixtures(),
        "reactions.json",
        r#"[{"user":{"login":"chatgpt-codex-connector[bot]"},"content":"+1","created_at":"2026-01-18T10:05:00Z"}]"#,
    );
    env.write_state(State {
        current_round: 0,
        max_iterations: 42,
        start_branch: env.branch(),
        codex_model: "gpt-5.4".to_string(),
        codex_effort: "medium".to_string(),
        codex_timeout: 900,
        pr_number: Some(123),
        configured_bots: Some(vec!["codex".to_string()]),
        active_bots: Some(vec!["codex".to_string()]),
        poll_interval: Some(1),
        poll_timeout: Some(2),
        started_at: Some("2026-01-18T10:00:00Z".to_string()),
        startup_case: Some("1".to_string()),
        latest_commit_sha: Some(env.head_sha()),
        latest_commit_at: Some("2026-01-18T10:00:00Z".to_string()),
        ..State::default()
    });
    env.write_resolve(0);

    let output = env.env_cmd().args(["stop", "pr"]).output().unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(env.loop_dir.join("approve-state.md").exists());
}

#[test]
fn stop_pr_missing_trigger_blocks_for_case4() {
    let env = PrStopEnv::new();
    env.write_state(State {
        current_round: 0,
        max_iterations: 42,
        start_branch: env.branch(),
        codex_model: "gpt-5.4".to_string(),
        codex_effort: "medium".to_string(),
        codex_timeout: 900,
        pr_number: Some(123),
        configured_bots: Some(vec!["claude".to_string()]),
        active_bots: Some(vec!["claude".to_string()]),
        poll_interval: Some(1),
        poll_timeout: Some(2),
        started_at: Some("2026-01-18T10:00:00Z".to_string()),
        startup_case: Some("4".to_string()),
        latest_commit_sha: Some(env.head_sha()),
        latest_commit_at: Some("2026-01-18T10:00:00Z".to_string()),
        ..State::default()
    });
    env.write_resolve(0);

    let output = env.env_cmd().args(["stop", "pr"]).output().unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"decision\":\"block\""));
    assert!(stdout.contains("trigger"));
    assert!(env.loop_dir.join("state.md").exists());
}

#[test]
fn stop_pr_comments_with_issues_advance_round_and_write_feedback() {
    let env = PrStopEnv::new();
    write_fixture(
        env.fixtures(),
        "pr-reviews.json",
        r#"[{"id":4001,"user":{"login":"chatgpt-codex-connector[bot]"},"submitted_at":"2026-01-18T11:15:00Z","body":"Please fix the edge case in parser","state":"COMMENTED"}]"#,
    );
    env.mock_codex(
        "#!/bin/bash\nif [[ \"$1\" == \"exec\" ]]; then\n  cat >/dev/null\n  printf '### Per-Bot Status\\n| Bot | Status | Summary |\\n|-----|--------|---------|\\n| codex | ISSUES | Parser edge case remains |\\n\\n### Issues Found\\n- Fix parser edge case handling\\n\\n### Approved Bots\\n\\n### Final Recommendation\\nISSUES_REMAINING\\n'\nfi\n",
    );
    env.write_state(State {
        current_round: 0,
        max_iterations: 42,
        start_branch: env.branch(),
        codex_model: "gpt-5.4".to_string(),
        codex_effort: "medium".to_string(),
        codex_timeout: 900,
        pr_number: Some(123),
        configured_bots: Some(vec!["codex".to_string()]),
        active_bots: Some(vec!["codex".to_string()]),
        poll_interval: Some(1),
        poll_timeout: Some(2),
        started_at: Some("2026-01-18T10:00:00Z".to_string()),
        startup_case: Some("2".to_string()),
        latest_commit_sha: Some(env.head_sha()),
        latest_commit_at: Some("2026-01-18T10:00:00Z".to_string()),
        ..State::default()
    });
    env.write_resolve(0);

    let output = env.env_cmd().args(["stop", "pr"]).output().unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Parser edge case remains"));
    assert!(env.loop_dir.join("round-1-pr-comment.md").exists());
    assert!(env.loop_dir.join("round-1-pr-check.md").exists());
    assert!(env.loop_dir.join("round-1-pr-feedback.md").exists());

    let state = fs::read_to_string(env.loop_dir.join("state.md")).unwrap();
    assert!(state.contains("current_round: 1"));
    assert!(state.contains("active_bots:"));
}

fn write_fixture(dir: &Path, name: &str, contents: &str) {
    fs::write(dir.join(name), contents).unwrap();
}

fn run(cmd: &mut Command) {
    let status = cmd.status().unwrap();
    assert!(status.success());
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }
}
