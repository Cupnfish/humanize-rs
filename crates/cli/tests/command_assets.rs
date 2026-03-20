use std::fs;
use std::path::{Path, PathBuf};

fn read_file(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|_| panic!("failed to read {}", path.display()))
}

fn normalize_command_semantics(content: &str) -> String {
    content
        .replace(
            "Bash(humanize setup rlcr:*)",
            "Bash(${CLAUDE_PLUGIN_ROOT}/scripts/setup-rlcr-loop.sh:*)",
        )
        .replace(
            "Bash(humanize setup pr:*)",
            "Bash(${CLAUDE_PLUGIN_ROOT}/scripts/setup-pr-loop.sh:*)",
        )
        .replace(
            "Bash(humanize cancel rlcr)",
            "Bash(${CLAUDE_PLUGIN_ROOT}/scripts/cancel-rlcr-loop.sh)",
        )
        .replace(
            "Bash(humanize cancel rlcr --force)",
            "Bash(${CLAUDE_PLUGIN_ROOT}/scripts/cancel-rlcr-loop.sh --force)",
        )
        .replace(
            "Bash(humanize cancel pr)",
            "Bash(${CLAUDE_PLUGIN_ROOT}/scripts/cancel-pr-loop.sh)",
        )
        .replace(
            "Bash(humanize cancel pr --force)",
            "Bash(${CLAUDE_PLUGIN_ROOT}/scripts/cancel-pr-loop.sh --force)",
        )
        .replace(
            "humanize setup rlcr $ARGUMENTS",
            "\"${CLAUDE_PLUGIN_ROOT}/scripts/setup-rlcr-loop.sh\" $ARGUMENTS",
        )
        .replace(
            "humanize setup pr $ARGUMENTS",
            "\"${CLAUDE_PLUGIN_ROOT}/scripts/setup-pr-loop.sh\" $ARGUMENTS",
        )
        .replace(
            "humanize cancel rlcr",
            "\"${CLAUDE_PLUGIN_ROOT}/scripts/cancel-rlcr-loop.sh\"",
        )
        .replace(
            "humanize cancel rlcr --force",
            "\"${CLAUDE_PLUGIN_ROOT}/scripts/cancel-rlcr-loop.sh\" --force",
        )
        .replace(
            "humanize cancel pr",
            "\"${CLAUDE_PLUGIN_ROOT}/scripts/cancel-pr-loop.sh\"",
        )
        .replace("Before running the setup command,", "Before running the setup script,")
        .replace(
            "let the setup command handle the error",
            "let the setup script handle the error",
        )
        .replace(
            "let the setup command handle path validation",
            "let the setup script handle path validation",
        )
        .replace(
            "continue to setup below",
            "continue to setup script below",
        )
        .replace(
            "execute the setup command to initialize the loop:",
            "execute the setup script to initialize the loop:",
        )
        .replace(
            "execute the setup command to initialize the PR review loop:",
            "execute the setup script to initialize the PR review loop:",
        )
        .replace(
            "Execute the setup command to initialize the PR review loop:",
            "Execute the setup script to initialize the PR review loop:",
        )
        .replace("Run the cancel command:", "Run the cancel script:")
        .replace(
            "**Key principle**: The command handles all cancellation logic.",
            "**Key principle**: The script handles all cancellation logic.",
        )
        .replace(
            "The loop directory with summaries, review results, and state information will be preserved for reference. The command writes `.cancel-requested` and renames the active state file to `cancel-state.md`.",
            "The loop directory with summaries, review results, and state information will be preserved for reference.",
        )
        .replace(
            "The loop directory with comments, resolution summaries, and state information will be preserved for reference. The command writes `.cancel-requested` and renames `state.md` to `cancel-state.md`.",
            "The loop directory with comments, resolution summaries, and state information will be preserved for reference.",
        )
        .replace(
            "The setup command provides the exact mention string to use (for example `@claude @codex`).\nUse whatever bot mentions are shown in the initial prompt. They match the flags you provided.",
            "The setup script provides the exact mention string to use (e.g., `@claude @codex`).\nUse whatever bot mentions are shown in the initial prompt - they match the flags you provided.",
        )
        .replace("--max-iterations, ", "")
        .replace("/humanize-rs:cancel-rlcr-loop", "/humanize:cancel-rlcr-loop")
        .replace("/humanize-rs:cancel-pr-loop", "/humanize:cancel-pr-loop")
        .replace("/humanize-rs:start-rlcr-loop", "/humanize:start-rlcr-loop")
        .replace("/humanize-rs:start-pr-loop", "/humanize:start-pr-loop")
        .replace("/humanize-rs:resume-rlcr-loop", "/humanize:resume-rlcr-loop")
        .replace("/humanize-rs:resume-pr-loop", "/humanize:resume-pr-loop")
        .replace("/humanize-rs:gen-plan", "/humanize:gen-plan")
}

#[test]
fn transport_normalized_commands_match_legacy_specs() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.join("../..");

    let pairs = [
        (
            "commands/start-rlcr-loop.md",
            "humanize/commands/start-rlcr-loop.md",
        ),
        (
            "commands/start-pr-loop.md",
            "humanize/commands/start-pr-loop.md",
        ),
        (
            "commands/cancel-rlcr-loop.md",
            "humanize/commands/cancel-rlcr-loop.md",
        ),
        (
            "commands/cancel-pr-loop.md",
            "humanize/commands/cancel-pr-loop.md",
        ),
    ];

    for (current, legacy) in pairs {
        let current_path = repo_root.join(current);
        let legacy_path = repo_root.join(legacy);
        let current_content = normalize_command_semantics(&read_file(&current_path));
        let legacy_content = read_file(&legacy_path);
        assert_eq!(
            current_content, legacy_content,
            "command asset drifted from legacy spec: {}",
            current
        );
    }
}
