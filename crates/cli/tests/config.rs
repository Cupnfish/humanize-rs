use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

struct ConfigEnv {
    _tempdir: TempDir,
    project_dir: PathBuf,
    user_config_home: PathBuf,
}

impl ConfigEnv {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let project_dir = root.join("project");
        let user_config_home = root.join("xdg");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&user_config_home).unwrap();
        Self {
            _tempdir: tempdir,
            project_dir,
            user_config_home,
        }
    }

    fn user_config_path(&self) -> PathBuf {
        self.user_config_home.join("humanize").join("config.json")
    }

    fn project_config_path(&self) -> PathBuf {
        self.project_dir.join(".humanize").join("config.json")
    }

    fn run_merged(&self) -> std::process::Output {
        Command::new(bin())
            .args(["config", "merged", "--json", "--with-meta"])
            .env("CLAUDE_PROJECT_DIR", self.project_dir.display().to_string())
            .env(
                "XDG_CONFIG_HOME",
                self.user_config_home.display().to_string(),
            )
            .current_dir(&self.project_dir)
            .output()
            .unwrap()
    }
}

#[test]
fn config_merged_returns_defaults_without_optional_files() {
    let env = ConfigEnv::new();
    let output = env.run_merged();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["merged"]["alternative_plan_language"], "");
    assert_eq!(json["merged"]["gen_plan_mode"], "discussion");
    assert_eq!(json["user_config_loaded"], false);
    assert_eq!(json["project_config_loaded"], false);
}

#[test]
fn config_merged_applies_user_then_project_overrides_and_tracks_explicit_keys() {
    let env = ConfigEnv::new();
    fs::create_dir_all(env.user_config_path().parent().unwrap()).unwrap();
    fs::create_dir_all(env.project_config_path().parent().unwrap()).unwrap();
    fs::write(
        env.user_config_path(),
        r#"{"gen_plan_mode":"direct","alternative_plan_language":"Japanese"}"#,
    )
    .unwrap();
    fs::write(
        env.project_config_path(),
        r#"{"gen_plan_mode":"discussion","chinese_plan":true}"#,
    )
    .unwrap();

    let output = env.run_merged();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["merged"]["gen_plan_mode"], "discussion");
    assert_eq!(json["merged"]["alternative_plan_language"], "Japanese");
    assert_eq!(json["merged"]["chinese_plan"], true);
    assert_eq!(
        json["explicit_user_keys"],
        serde_json::json!(["alternative_plan_language", "gen_plan_mode"])
    );
    assert_eq!(
        json["explicit_project_keys"],
        serde_json::json!(["chinese_plan", "gen_plan_mode"])
    );
}

#[test]
fn config_merged_warns_and_ignores_malformed_optional_layers() {
    let env = ConfigEnv::new();
    fs::create_dir_all(env.user_config_path().parent().unwrap()).unwrap();
    fs::write(env.user_config_path(), r#"["not-an-object"]"#).unwrap();

    let output = env.run_merged();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Warning: ignoring malformed user config"));

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["merged"]["gen_plan_mode"], "discussion");
    assert_eq!(json["user_config_loaded"], false);
}
