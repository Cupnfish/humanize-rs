use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct PrSetupEnv {
    _tempdir: TempDir,
    project_dir: PathBuf,
    bin_dir: PathBuf,
    fixtures_dir: PathBuf,
}

impl PrSetupEnv {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let project_dir = root.join("project");
        let bin_dir = root.join("bin");
        let fixtures_dir = root.join("fixtures");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&fixtures_dir).unwrap();

        let mock_gh_src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/mocks/gh");
        fs::copy(&mock_gh_src, bin_dir.join("gh")).unwrap();
        make_executable(&bin_dir.join("gh"));

        fs::write(bin_dir.join("codex"), "#!/bin/bash\nexit 0\n").unwrap();
        make_executable(&bin_dir.join("codex"));

        fs::write(fixtures_dir.join("issue-comments.json"), "[]").unwrap();
        fs::write(fixtures_dir.join("review-comments.json"), "[]").unwrap();
        fs::write(fixtures_dir.join("pr-reviews.json"), "[]").unwrap();
        fs::write(fixtures_dir.join("reactions.json"), "[]").unwrap();
        fs::write(fixtures_dir.join("comment-reactions.json"), "[]").unwrap();

        run(Command::new("git")
            .args(["init", "-q"])
            .current_dir(&project_dir));
        run(Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&project_dir));
        run(Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&project_dir));
        fs::write(project_dir.join("README.md"), "# temp\n").unwrap();
        run(Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&project_dir));
        run(Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(&project_dir));

        Self {
            _tempdir: tempdir,
            project_dir,
            bin_dir,
            fixtures_dir,
        }
    }

    fn path_env(&self) -> String {
        format!(
            "{}:{}",
            self.bin_dir.display(),
            std::env::var("PATH").unwrap()
        )
    }

    fn project(&self) -> &Path {
        &self.project_dir
    }

    fn fixtures(&self) -> &Path {
        &self.fixtures_dir
    }
}

#[test]
fn setup_pr_creates_state_goal_tracker_and_prompt() {
    let env = PrSetupEnv::new();

    let output = Command::new(bin())
        .args(["setup", "pr", "--codex"])
        .env("PATH", env.path_env())
        .env("MOCK_GH_FIXTURES_DIR", env.fixtures().display().to_string())
        .env("MOCK_GH_PR_NUMBER", "999")
        .env("MOCK_GH_PR_STATE", "OPEN")
        .env("MOCK_GH_LATEST_COMMIT_AT", "2026-01-18T10:00:00Z")
        .env("MOCK_GH_HEAD_SHA", "abc123xyz")
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .output()
        .unwrap();

    assert!(output.status.success());
    let loop_dir = first_loop_dir(env.project().join(".humanize/pr-loop"));
    assert!(loop_dir.join("state.md").exists());
    assert!(loop_dir.join("goal-tracker.md").exists());
    assert!(loop_dir.join("round-0-prompt.md").exists());
    assert!(loop_dir.join("round-0-pr-comment.md").exists());

    let goal = fs::read_to_string(loop_dir.join("goal-tracker.md")).unwrap();
    assert!(goal.contains("Issue Summary"));
    assert!(goal.contains("999"));

    let state = fs::read_to_string(loop_dir.join("state.md")).unwrap();
    assert!(state.contains("configured_bots:"));
    assert!(state.contains("active_bots:"));
    assert!(state.contains("startup_case: '1'") || state.contains("startup_case: 1"));
}

#[test]
fn setup_pr_case45_missing_trigger_id_fails_and_cleans_loop_dir() {
    let env = PrSetupEnv::new();
    fs::write(
        env.fixtures().join("issue-comments.json"),
        r#"[{"id":1001,"user":{"login":"claude[bot]"},"created_at":"2026-01-18T08:00:00Z","body":"Issue found"}]"#,
    )
    .unwrap();

    let output = Command::new(bin())
        .args(["setup", "pr", "--claude"])
        .env("PATH", env.path_env())
        .env("MOCK_GH_FIXTURES_DIR", env.fixtures().display().to_string())
        .env("MOCK_GH_PR_NUMBER", "123")
        .env("MOCK_GH_PR_STATE", "OPEN")
        .env("MOCK_GH_LATEST_COMMIT_AT", "2026-01-18T12:00:00Z")
        .env("MOCK_GH_HEAD_SHA", "abc123xyz")
        .env("MOCK_GH_COMMENT_ID_LOOKUP_FAIL", "true")
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("trigger comment ID"));
    assert!(
        fs::read_dir(env.project().join(".humanize/pr-loop"))
            .map(|mut it| it.next().is_none())
            .unwrap_or(true)
    );
}

#[test]
fn setup_pr_formats_round0_comments_like_bash_output() {
    let env = PrSetupEnv::new();
    fs::write(
        env.fixtures().join("issue-comments.json"),
        r#"[
  {"id":1001,"user":{"login":"alice","type":"User"},"created_at":"2026-01-18T11:00:00Z","updated_at":"2026-01-18T11:00:00Z","body":"Human feedback"},
  {"id":1002,"user":{"login":"claude[bot]","type":"Bot"},"created_at":"2026-01-18T11:10:00Z","updated_at":"2026-01-18T11:10:00Z","body":"Bot summary comment"}
]"#,
    )
    .unwrap();
    fs::write(
        env.fixtures().join("review-comments.json"),
        r#"[
  {"id":2001,"user":{"login":"chatgpt-codex-connector[bot]","type":"Bot"},"created_at":"2026-01-18T11:20:00Z","updated_at":"2026-01-18T11:20:00Z","body":"Inline review","path":"src/lib.rs","line":42}
]"#,
    )
    .unwrap();
    fs::write(
        env.fixtures().join("pr-reviews.json"),
        r#"[
  {"id":3001,"user":{"login":"chatgpt-codex-connector[bot]","type":"Bot"},"submitted_at":"2026-01-18T11:30:00Z","body":"","state":"APPROVED"}
]"#,
    )
    .unwrap();

    let output = Command::new(bin())
        .args(["setup", "pr", "--claude", "--codex"])
        .env("PATH", env.path_env())
        .env("MOCK_GH_FIXTURES_DIR", env.fixtures().display().to_string())
        .env("MOCK_GH_PR_NUMBER", "456")
        .env("MOCK_GH_PR_STATE", "OPEN")
        .env("MOCK_GH_LATEST_COMMIT_AT", "2026-01-18T10:00:00Z")
        .env("MOCK_GH_HEAD_SHA", "head456")
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .output()
        .unwrap();

    assert!(output.status.success());
    let loop_dir = first_loop_dir(env.project().join(".humanize/pr-loop"));
    let comments = fs::read_to_string(loop_dir.join("round-0-pr-comment.md")).unwrap();
    assert!(
        comments.contains("# PR Comments for #456"),
        "comments={comments}"
    );
    assert!(
        comments.contains("## Human Comments"),
        "comments={comments}"
    );
    assert!(
        comments.contains("## Bot Comments (Grouped by Bot)"),
        "comments={comments}"
    );
    assert!(
        comments.contains("### Comment from alice"),
        "comments={comments}"
    );
    assert!(
        comments.contains("### Comments from claude[bot]"),
        "comments={comments}"
    );
    assert!(
        comments.contains("### Comments from chatgpt-codex-connector[bot]"),
        "comments={comments}"
    );
    assert!(
        comments.contains("- **File**: `src/lib.rs` (line 42)"),
        "comments={comments}"
    );
    assert!(
        comments.contains("- **Status**: APPROVED"),
        "comments={comments}"
    );
    assert!(
        comments.contains("[Review state: APPROVED]"),
        "comments={comments}"
    );
}

#[test]
fn setup_pr_case45_reuses_existing_trigger_and_preserves_comment_id() {
    let env = PrSetupEnv::new();
    fs::write(
        env.fixtures().join("issue-comments.json"),
        r#"[
  {"id":1001,"user":{"login":"claude[bot]","type":"Bot"},"created_at":"2026-01-18T08:00:00Z","updated_at":"2026-01-18T08:00:00Z","body":"Issue found"},
  {"id":2002,"user":{"login":"testuser","type":"User"},"created_at":"2026-01-18T12:05:00Z","updated_at":"2026-01-18T12:05:00Z","body":"@claude please review the latest changes (new commits since last review)"}
]"#,
    )
    .unwrap();
    fs::write(
        env.fixtures().join("comment-reactions.json"),
        r#"[
  {"user":{"login":"claude[bot]"},"content":"eyes","created_at":"2026-01-18T12:05:10Z"}
]"#,
    )
    .unwrap();

    let output = Command::new(bin())
        .args(["setup", "pr", "--claude"])
        .env("PATH", env.path_env())
        .env("MOCK_GH_FIXTURES_DIR", env.fixtures().display().to_string())
        .env("MOCK_GH_PR_NUMBER", "123")
        .env("MOCK_GH_PR_STATE", "OPEN")
        .env("MOCK_GH_LATEST_COMMIT_AT", "2026-01-18T12:00:00Z")
        .env("MOCK_GH_HEAD_SHA", "abc123xyz")
        .env("MOCK_GH_COMMENT_ID_LOOKUP_FAIL", "true")
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let loop_dir = first_loop_dir(env.project().join(".humanize/pr-loop"));
    let state = fs::read_to_string(loop_dir.join("state.md")).unwrap();
    assert!(
        state.contains("trigger_comment_id: '2002'") || state.contains("trigger_comment_id: 2002"),
        "state={state}"
    );
    assert!(
        state.contains("last_trigger_at: 2026-01-18T12:05:00Z"),
        "state={state}"
    );
}

#[test]
fn setup_pr_falls_back_to_case1_when_commit_info_fetch_fails() {
    let env = PrSetupEnv::new();
    fs::write(
        env.fixtures().join("issue-comments.json"),
        r#"[{"id":1001,"user":{"login":"claude[bot]","type":"Bot"},"created_at":"2026-01-18T08:00:00Z","updated_at":"2026-01-18T08:00:00Z","body":"Issue found"}]"#,
    )
    .unwrap();

    let output = Command::new(bin())
        .args(["setup", "pr", "--claude"])
        .env("PATH", env.path_env())
        .env("MOCK_GH_FIXTURES_DIR", env.fixtures().display().to_string())
        .env("MOCK_GH_PR_NUMBER", "777")
        .env("MOCK_GH_PR_STATE", "OPEN")
        .env("MOCK_GH_FAIL_PR_COMMIT_INFO", "true")
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let loop_dir = first_loop_dir(env.project().join(".humanize/pr-loop"));
    let state = fs::read_to_string(loop_dir.join("state.md")).unwrap();
    assert!(state.contains("startup_case: '1'") || state.contains("startup_case: 1"));
    assert!(
        state.contains(&git_head(env.project())),
        "state should fall back to git HEAD: {state}"
    );
}

#[test]
fn setup_pr_round0_comments_include_api_warning_when_comment_fetch_is_partial() {
    let env = PrSetupEnv::new();

    let output = Command::new(bin())
        .args(["setup", "pr", "--codex"])
        .env("PATH", env.path_env())
        .env("MOCK_GH_FIXTURES_DIR", env.fixtures().display().to_string())
        .env("MOCK_GH_PR_NUMBER", "888")
        .env("MOCK_GH_PR_STATE", "OPEN")
        .env("MOCK_GH_LATEST_COMMIT_AT", "2026-01-18T10:00:00Z")
        .env("MOCK_GH_HEAD_SHA", "head888")
        .env("MOCK_GH_FAIL_ISSUE_COMMENTS", "true")
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let loop_dir = first_loop_dir(env.project().join(".humanize/pr-loop"));
    let comments = fs::read_to_string(loop_dir.join("round-0-pr-comment.md")).unwrap();
    assert!(
        comments.contains("**Warning:** Some API calls failed. Comments may be incomplete."),
        "comments={comments}"
    );
}

#[test]
fn setup_pr_uses_repo_from_autodetected_pr_url() {
    let env = PrSetupEnv::new();

    let output = Command::new(bin())
        .args(["setup", "pr", "--codex"])
        .env("PATH", env.path_env())
        .env("MOCK_GH_FIXTURES_DIR", env.fixtures().display().to_string())
        .env("MOCK_GH_CURRENT_REPO", "forkowner/forkrepo")
        .env(
            "MOCK_GH_PR_URL",
            "https://github.com/upstream/project/pull/999",
        )
        .env("MOCK_GH_EXPECT_REPO", "upstream/project")
        .env("MOCK_GH_PR_NUMBER", "999")
        .env("MOCK_GH_PR_STATE", "OPEN")
        .env("MOCK_GH_LATEST_COMMIT_AT", "2026-01-18T10:00:00Z")
        .env("MOCK_GH_HEAD_SHA", "head999")
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let loop_dir = first_loop_dir(env.project().join(".humanize/pr-loop"));
    let comments = fs::read_to_string(loop_dir.join("round-0-pr-comment.md")).unwrap();
    assert!(
        comments.contains("Repository: upstream/project"),
        "comments={comments}"
    );
}

fn first_loop_dir(base: PathBuf) -> PathBuf {
    fs::read_dir(base)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .unwrap()
}

fn run(cmd: &mut Command) {
    let status = cmd.status().unwrap();
    assert!(status.success());
}

fn git_head(project_dir: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
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
