use std::fs;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

#[test]
fn install_target_codex_syncs_only_skill_files() {
    let tempdir = tempfile::tempdir().unwrap();
    let skills_dir = tempdir.path().join("skills");

    let output = Command::new(bin())
        .args([
            "install",
            "--target",
            "codex",
            "--skills-dir",
            skills_dir.to_str().unwrap(),
        ])
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
    assert!(!skills_dir.join("humanize/prompt-template").exists());
    assert!(!skills_dir.join("humanize/bin").exists());
    assert!(!skills_dir.join("humanize/scripts").exists());

    let skill = fs::read_to_string(skills_dir.join("humanize-rlcr/SKILL.md")).unwrap();
    assert!(skill.contains("humanize"));
    assert!(skill.contains("gate rlcr"));
    assert!(!skill.contains("CLAUDE_PLUGIN_ROOT"));
    assert!(!skill.contains("HUMANIZE_RUNTIME_ROOT"));
}

#[test]
fn install_target_kimi_dry_run_does_not_write() {
    let tempdir = tempfile::tempdir().unwrap();
    let skills_dir = tempdir.path().join("skills");

    let output = Command::new(bin())
        .args([
            "install",
            "--target",
            "kimi",
            "--skills-dir",
            skills_dir.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!skills_dir.exists());
}
