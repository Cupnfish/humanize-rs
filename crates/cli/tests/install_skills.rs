use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

#[test]
fn install_skills_syncs_runtime_and_hydrates_skill_files() {
    let tempdir = tempfile::tempdir().unwrap();
    let skills_dir = tempdir.path().join("skills");
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");

    let output = Command::new(bin())
        .args([
            "install",
            "--target",
            "codex",
            "--skills-dir",
            skills_dir.to_str().unwrap(),
        ])
        .env("CLAUDE_PLUGIN_ROOT", plugin_root.display().to_string())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(skills_dir.join("humanize").exists());
    assert!(skills_dir.join("ask-codex").exists());
    assert!(skills_dir.join("humanize-gen-plan").exists());
    assert!(skills_dir.join("humanize-rlcr").exists());
    assert!(skills_dir.join("humanize/prompt-template").exists());
    assert!(!skills_dir.join("humanize/bin").exists());
    assert!(!skills_dir.join("humanize/scripts").exists());

    let skill = fs::read_to_string(skills_dir.join("humanize-rlcr/SKILL.md")).unwrap();
    assert!(skill.contains("CLAUDE_PLUGIN_ROOT="));
    assert!(skill.contains("humanize"));
    assert!(skill.contains("gate rlcr"));
    assert!(skill.contains(skills_dir.join("humanize").to_str().unwrap()));
    assert!(!skill.contains("scripts/rlcr-stop-gate.sh"));
}

#[test]
fn install_skills_dry_run_does_not_write() {
    let tempdir = tempfile::tempdir().unwrap();
    let skills_dir = tempdir.path().join("skills");
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");

    let output = Command::new(bin())
        .args([
            "install",
            "--target",
            "kimi",
            "--skills-dir",
            skills_dir.to_str().unwrap(),
            "--dry-run",
        ])
        .env("CLAUDE_PLUGIN_ROOT", plugin_root.display().to_string())
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!skills_dir.exists());
}
