use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct RlcrSetupEnv {
    _tempdir: TempDir,
    project_dir: PathBuf,
    bin_dir: PathBuf,
}

impl RlcrSetupEnv {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let project_dir = tempdir.path().join("project");
        let bin_dir = tempdir.path().join("bin");
        fs::create_dir_all(project_dir.join("docs")).unwrap();
        fs::create_dir_all(project_dir.join("work")).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();

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
        fs::write(project_dir.join(".gitignore"), "docs/\n.humanize/\n").unwrap();
        run(Command::new("git")
            .args(["add", "README.md", ".gitignore"])
            .current_dir(&project_dir));
        run(Command::new("git")
            .args(["commit", "-q", "-m", "init"])
            .current_dir(&project_dir));

        fs::write(
            project_dir.join("docs/plan.md"),
            "# Plan\n\nGoal\n\nRequirement A\nRequirement B\nRequirement C\n",
        )
        .unwrap();

        Self {
            _tempdir: tempdir,
            project_dir,
            bin_dir,
        }
    }

    fn project(&self) -> &Path {
        &self.project_dir
    }

    fn noncanonical_project_root(&self) -> PathBuf {
        self.project_dir.join("work").join("..")
    }

    fn path_env(&self) -> String {
        format!(
            "{}:{}",
            self.bin_dir.display(),
            std::env::var("PATH").unwrap()
        )
    }

    fn mock_codex(&self, script: &str) {
        let path = self.bin_dir.join("codex");
        fs::write(&path, script).unwrap();
        make_executable(&path);
    }
}

#[test]
fn setup_rlcr_accepts_relative_plan_file_from_noncanonical_project_root() {
    let env = RlcrSetupEnv::new();

    let output = Command::new(bin())
        .args(["setup", "rlcr", "docs/plan.md"])
        .env(
            "CLAUDE_PROJECT_DIR",
            env.noncanonical_project_root().display().to_string(),
        )
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let loop_dir = first_loop_dir(env.project().join(".humanize/rlcr"));
    assert!(loop_dir.join("state.md").exists());
    assert!(loop_dir.join("plan.md").exists());

    let state = fs::read_to_string(loop_dir.join("state.md")).unwrap();
    assert!(state.contains("plan_file: docs/plan.md"), "state={state}");
    assert!(state.contains("plan_mode: snapshot"), "state={state}");
    assert!(
        state.contains("plan_source_path: docs/plan.md"),
        "state={state}"
    );
    assert!(state.contains("plan_snapshot_path:"), "state={state}");

    let plan_metadata = fs::symlink_metadata(loop_dir.join("plan.md")).unwrap();
    assert!(!plan_metadata.file_type().is_symlink());
}

#[test]
fn setup_rlcr_without_plan_uses_pending_artifact_plan() {
    let env = RlcrSetupEnv::new();
    let counter_file = env.project().join("codex-count");
    fs::write(
        env.project().join("draft.md"),
        "# Parser Draft\n\nNeed a parser.\n",
    )
    .unwrap();
    env.mock_codex(
        "#!/bin/bash\ncount_file=\"${MOCK_CODEX_COUNTER_FILE:?}\"\ncount=0\n[[ -f \"$count_file\" ]] && count=$(cat \"$count_file\")\ncount=$((count + 1))\nprintf '%s' \"$count\" > \"$count_file\"\nif [[ \"$1\" == \"exec\" ]]; then\n  cat >/dev/null\n  if [[ \"$count\" == \"1\" ]]; then\n    printf 'RELEVANT: parser work matches this repository\\n'\n  elif [[ \"$count\" == \"2\" ]]; then\n    printf '{\"issues\":[],\"metrics\":[],\"mixed_languages\":false,\"language_candidates\":[],\"notes\":[]}'\n  else\n    printf '# Parser Plan\\n\\n## Goal Description\\nBuild the parser.\\n\\n## Acceptance Criteria\\n- AC-1: Parse valid inputs\\n'\n  fi\nfi\n",
    );

    let draft = Command::new(bin())
        .args([
            "gen-draft",
            "--input",
            "draft.md",
            "--title",
            "Parser Draft",
        ])
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .env("CLAUDE_SESSION_ID", "thread-setup")
        .current_dir(env.project())
        .output()
        .unwrap();
    assert!(draft.status.success());

    let plan = Command::new(bin())
        .args(["gen-plan"])
        .env("PATH", env.path_env())
        .env(
            "MOCK_CODEX_COUNTER_FILE",
            counter_file.display().to_string(),
        )
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .env("CLAUDE_SESSION_ID", "thread-setup")
        .current_dir(env.project())
        .output()
        .unwrap();
    assert!(plan.status.success());
    fs::remove_file(env.project().join("draft.md")).unwrap();
    fs::remove_file(&counter_file).unwrap();

    let setup = Command::new(bin())
        .args(["setup", "rlcr"])
        .env("CLAUDE_PROJECT_DIR", env.project().display().to_string())
        .env("CLAUDE_SESSION_ID", "thread-setup")
        .current_dir(env.project())
        .output()
        .unwrap();

    assert!(
        setup.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&setup.stdout),
        String::from_utf8_lossy(&setup.stderr)
    );

    let loop_dir = first_loop_dir(env.project().join(".humanize/rlcr"));
    let state = fs::read_to_string(loop_dir.join("state.md")).unwrap();
    assert!(
        state.contains("plan_file: .humanize/planning/plans/"),
        "state={state}"
    );
    assert!(state.contains("source_plan_id:"), "state={state}");
    assert!(state.contains("source_plan_revision: 1"), "state={state}");

    let index = read_json(env.project().join(".humanize/planning/index.json"));
    let plans = index.get("plans").and_then(Value::as_array).unwrap();
    assert_eq!(plans[0].get("status").and_then(Value::as_str), Some("used"));
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

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }
}

fn read_json(path: PathBuf) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}
