use std::fs;
use std::path::{Path, PathBuf};

fn read_file(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|_| panic!("failed to read {}", path.display()))
}

#[test]
fn command_files_use_native_cli_not_legacy_scripts() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.join("../..");

    let command_files = [
        "commands/start-rlcr-loop.md",
        "commands/start-pr-loop.md",
        "commands/cancel-rlcr-loop.md",
        "commands/cancel-pr-loop.md",
        "commands/resume-rlcr-loop.md",
        "commands/resume-pr-loop.md",
        "commands/gen-plan.md",
    ];

    let legacy_patterns = [
        "setup-rlcr-loop.sh",
        "setup-pr-loop.sh",
        "cancel-rlcr-loop.sh",
        "cancel-pr-loop.sh",
        "${CLAUDE_PLUGIN_ROOT}/scripts/",
        "{{HUMANIZE_RUNTIME_ROOT}}/scripts/",
    ];

    for file in &command_files {
        let path = repo_root.join(file);
        let content = read_file(&path);
        for pattern in &legacy_patterns {
            assert!(
                !content.contains(pattern),
                "command file {} still references legacy script: {}",
                file,
                pattern
            );
        }
    }
}

#[test]
fn command_files_have_valid_frontmatter() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.join("../..");

    let command_files = [
        "commands/start-rlcr-loop.md",
        "commands/start-pr-loop.md",
        "commands/cancel-rlcr-loop.md",
        "commands/cancel-pr-loop.md",
        "commands/resume-rlcr-loop.md",
        "commands/resume-pr-loop.md",
        "commands/gen-plan.md",
    ];

    for file in &command_files {
        let path = repo_root.join(file);
        let content = read_file(&path);
        assert!(
            content.starts_with("---"),
            "command file {} missing YAML frontmatter delimiter",
            file
        );
        assert!(
            content.contains("description:"),
            "command file {} missing 'description' in frontmatter",
            file
        );
    }
}

#[test]
fn loop_command_files_reference_humanize_cli() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.join("../..");

    let loop_commands = [
        "commands/start-rlcr-loop.md",
        "commands/start-pr-loop.md",
        "commands/cancel-rlcr-loop.md",
        "commands/cancel-pr-loop.md",
        "commands/resume-rlcr-loop.md",
        "commands/resume-pr-loop.md",
    ];

    for file in &loop_commands {
        let path = repo_root.join(file);
        let content = read_file(&path);
        assert!(
            content.contains("humanize "),
            "command file {} should reference the native 'humanize' CLI",
            file
        );
    }
}
