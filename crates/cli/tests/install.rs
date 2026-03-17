use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_humanize")
}

#[test]
fn install_syncs_runtime_assets_into_plugin_root() {
    let tempdir = tempfile::tempdir().unwrap();
    let plugin_root = tempdir.path().join("plugin");
    let source_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");

    let output = Command::new(bin())
        .args([
            "install",
            "--plugin-root",
            plugin_root.to_str().unwrap(),
        ])
        .env("CLAUDE_PLUGIN_ROOT", source_root.display().to_string())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(plugin_root.join("prompt-template").exists());
    assert!(plugin_root.join("skills").exists());
    assert!(plugin_root.join("hooks/hooks.json").exists());
    assert!(plugin_root.join("commands").exists());
    assert!(!plugin_root.join("bin").exists());
}
