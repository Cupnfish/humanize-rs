use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct RlcrSetupEnv {
    _tempdir: TempDir,
    project_dir: PathBuf,
}

impl RlcrSetupEnv {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let project_dir = tempdir.path().join("project");
        fs::create_dir_all(project_dir.join("docs")).unwrap();
        fs::create_dir_all(project_dir.join("work")).unwrap();

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
        }
    }

    fn project(&self) -> &Path {
        &self.project_dir
    }

    fn noncanonical_project_root(&self) -> PathBuf {
        self.project_dir.join("work").join("..")
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
    assert!(state.contains("plan_source_path: docs/plan.md"), "state={state}");
    assert!(state.contains("plan_snapshot_path:"), "state={state}");

    let plan_metadata = fs::symlink_metadata(loop_dir.join("plan.md")).unwrap();
    assert!(!plan_metadata.file_type().is_symlink());
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
