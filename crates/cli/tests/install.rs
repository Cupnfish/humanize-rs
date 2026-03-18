use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

#[test]
fn install_syncs_runtime_assets_into_plugin_root() {
    let tempdir = tempfile::tempdir().unwrap();
    let plugin_root = tempdir.path().join("plugin");

    let output = Command::new(bin())
        .args([
            "install",
            "--target",
            "claude",
            "--plugin-root",
            plugin_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(plugin_root.join("hooks/hooks.json").exists());
    assert!(plugin_root.join("commands").exists());
    assert!(plugin_root.join(".claude-plugin/plugin.json").exists());
    assert!(!plugin_root.join("prompt-template").exists());
    assert!(!plugin_root.join("skills").exists());
}

#[test]
fn install_without_plugin_root_uses_global_data_dir() {
    let tempdir = tempfile::tempdir().unwrap();
    let project_dir = tempdir.path().join("project");
    let data_home = tempdir.path().join("data-home");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::create_dir_all(&data_home).unwrap();

    let output = Command::new(bin())
        .args(["install"])
        .env("XDG_DATA_HOME", data_home.display().to_string())
        .current_dir(&project_dir)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let expected_root = data_home.join("humanize-rs");
    assert!(expected_root.join("hooks").exists());
    assert!(expected_root.join(".claude-plugin").exists());
    assert!(!expected_root.join("prompt-template").exists());
    assert!(!project_dir.join("prompt-template").exists());
}
