//! Command handlers for the Humanize CLI.

use anyhow::{bail, Result};

use crate::hook_input::{get_command, get_file_path, read_hook_input, HookInput, HookOutput};
use crate::{CancelCommands, HookCommands, SetupCommands, StopCommands};

/// Handle setup commands.
pub fn handle_setup(cmd: SetupCommands) -> Result<()> {
    match cmd {
        SetupCommands::Rlcr {
            plan_file,
            max_iterations,
            base_branch,
            codex_model,
            agent_teams,
        } => {
            // TODO: Implement full RLCR setup
            // For now, call out to the shell script
            let project_root = std::env::var("CLAUDE_PROJECT_DIR")
                .unwrap_or_else(|_| std::env::current_dir().unwrap().to_string_lossy().into_owned());

            let mut args = vec![
                plan_file.clone(),
                "--max".to_string(),
                max_iterations.to_string(),
                "--codex-model".to_string(),
                format!("{}:high", codex_model),
            ];

            if agent_teams {
                args.push("--agent-teams".to_string());
            }

            if let Some(ref bb) = base_branch {
                args.extend(vec!["--base-branch".to_string(), bb.clone()]);
            }

            // Find the setup script
            let plugin_root = std::env::var("CLAUDE_PLUGIN_ROOT")
                .unwrap_or_else(|_| "/home/cupnfish/.claude/plugins/cache/humania/humanize/1.15.0".to_string());

            let setup_script = format!("{}/humanize/scripts/setup-rlcr-loop.sh", plugin_root);

            // Execute the shell script
            let status = std::process::Command::new(&setup_script)
                .args(&args)
                .current_dir(&project_root)
                .status()?;

            if !status.success() {
                bail!("RLCR setup failed with exit code: {:?}", status.code());
            }

            Ok(())
        }
        SetupCommands::Pr { pr_url } => {
            // TODO: Implement PR loop setup
            bail!("PR loop setup not yet implemented in Rust: {}", pr_url);
        }
    }
}

/// Handle cancel commands.
pub fn handle_cancel(cmd: CancelCommands) -> Result<()> {
    match cmd {
        CancelCommands::Rlcr => {
            let project_root = std::env::var("CLAUDE_PROJECT_DIR")
                .unwrap_or_else(|_| std::env::current_dir().unwrap().to_string_lossy().into_owned());

            let plugin_root = std::env::var("CLAUDE_PLUGIN_ROOT")
                .unwrap_or_else(|_| "/home/cupnfish/.claude/plugins/cache/humania/humanize/1.15.0".to_string());

            let cancel_script = format!("{}/humanize/scripts/cancel-rlcr-loop.sh", plugin_root);

            let status = std::process::Command::new(&cancel_script)
                .current_dir(&project_root)
                .status()?;

            if !status.success() {
                bail!("RLCR cancel failed with exit code: {:?}", status.code());
            }

            Ok(())
        }
        CancelCommands::Pr => {
            bail!("PR loop cancel not yet implemented in Rust");
        }
    }
}

/// Handle hook commands - all read JSON from stdin.
pub fn handle_hook(cmd: HookCommands) -> Result<()> {
    // Read hook input from stdin
    let input = match read_hook_input() {
        Ok(i) => i,
        Err(e) => {
            // If stdin is empty or invalid, allow by default
            eprintln!("Warning: Could not parse hook input: {}", e);
            HookOutput::allow().print();
            return Ok(());
        }
    };

    let result = match cmd {
        HookCommands::ReadValidator => validate_read(&input),
        HookCommands::WriteValidator => validate_write(&input),
        HookCommands::EditValidator => validate_edit(&input),
        HookCommands::BashValidator => validate_bash(&input),
        HookCommands::PlanFileValidator => validate_plan_file(&input),
        HookCommands::PostToolUse => handle_post_tool_use(&input),
    };

    result.print();
    Ok(())
}

/// Validate Read tool access.
fn validate_read(input: &HookInput) -> HookOutput {
    let file_path = match get_file_path(input) {
        Some(p) => p,
        None => return HookOutput::allow(), // No file_path, allow
    };

    let path_lower = file_path.to_lowercase();

    // Check for round-specific files
    if is_round_specific_file(&path_lower) {
        // TODO: Implement full allowlist logic matching shell
        // For now, block round files
        return HookOutput::block(format!(
            "Reading round-specific files is not allowed: {}",
            file_path
        ));
    }

    HookOutput::allow()
}

/// Validate Write tool access.
fn validate_write(input: &HookInput) -> HookOutput {
    let file_path = match get_file_path(input) {
        Some(p) => p,
        None => return HookOutput::allow(),
    };

    let path_lower = file_path.to_lowercase();

    // Check for protected state files
    if is_protected_state_file(&path_lower) {
        return HookOutput::block(format!(
            "Writing to protected state files is not allowed: {}",
            file_path
        ));
    }

    HookOutput::allow()
}

/// Validate Edit tool access.
fn validate_edit(input: &HookInput) -> HookOutput {
    let file_path = match get_file_path(input) {
        Some(p) => p,
        None => return HookOutput::allow(),
    };

    let path_lower = file_path.to_lowercase();

    // Check for protected state files
    if is_protected_state_file(&path_lower) {
        return HookOutput::block(format!(
            "Editing protected state files is not allowed: {}",
            file_path
        ));
    }

    // Check for goal-tracker protection after Round 0
    // TODO: Implement full goal-tracker protection

    HookOutput::allow()
}

/// Validate Bash command execution.
fn validate_bash(input: &HookInput) -> HookOutput {
    let command = match get_command(input) {
        Some(c) => c,
        None => return HookOutput::allow(),
    };

    let cmd_lower = command.to_lowercase();
    let cmd_trimmed = command.trim();

    // Safe command patterns
    let safe_patterns = [
        "git status", "git log", "git diff", "git branch", "git rev-parse",
        "cargo build", "cargo check", "cargo test", "cargo clippy", "cargo fmt --check",
        "echo ", "ls ", "cat ", "head ", "tail ", "grep ", "which ", "pwd",
    ];

    for pattern in &safe_patterns {
        if cmd_lower.starts_with(&pattern.to_lowercase()) {
            return HookOutput::allow();
        }
    }

    // Dangerous patterns
    let dangerous_patterns = [
        "rm ", "rm\t", "rmdir", "mv ", "mv\t", "cp ", "> ", ">>", "2>",
        "| ", " && ", "; ", "`", "$(", "chmod", "chown", "mkdir -p",
    ];

    for pattern in &dangerous_patterns {
        if cmd_trimmed.contains(pattern) {
            return HookOutput::block(format!(
                "Command contains potentially dangerous pattern '{}': {}",
                pattern, cmd_trimmed
            ));
        }
    }

    HookOutput::allow()
}

/// Validate plan file path.
fn validate_plan_file(input: &HookInput) -> HookOutput {
    // The plan-file validator is actually a UserPromptSubmit hook
    // that checks git state consistency, not just the plan path.
    // For now, allow everything.
    // TODO: Implement full git state validation

    HookOutput::allow()
}

/// Handle PostToolUse hook for session handshake.
fn handle_post_tool_use(input: &HookInput) -> HookOutput {
    // Only process Bash tool
    if input.tool_name != "Bash" {
        return HookOutput::allow();
    }

    let project_root = match std::env::var("CLAUDE_PROJECT_DIR") {
        Ok(p) => p,
        Err(_) => return HookOutput::allow(),
    };

    let pending_file = format!("{}/.humanize/.pending-session-id", project_root);

    // Check if pending signal file exists
    if !std::path::Path::new(&pending_file).exists() {
        return HookOutput::allow();
    }

    // Read the signal file
    let content = match std::fs::read_to_string(&pending_file) {
        Ok(c) => c,
        Err(_) => return HookOutput::allow(),
    };

    let lines: Vec<&str> = content.lines().collect();
    if lines.len() < 2 {
        return HookOutput::allow();
    }

    let state_file = lines[0];
    let _expected_cmd = lines[1];

    // Check if the bash command matches
    let command = get_command(input).unwrap_or_default();
    if !command.contains(_expected_cmd) {
        return HookOutput::allow();
    }

    // Get session_id
    let session_id = match &input.session_id {
        Some(s) => s.clone(),
        None => return HookOutput::allow(),
    };

    // Update state file with session_id
    // TODO: Implement proper state file patching

    // Remove signal file
    let _ = std::fs::remove_file(&pending_file);

    HookOutput::allow()
}

/// Check if path is a round-specific file.
fn is_round_specific_file(path_lower: &str) -> bool {
    // Check for round-N-*.md pattern
    if path_lower.contains("/round-") || path_lower.starts_with("round-") {
        let parts: Vec<&str> = path_lower.split('/').collect();
        if let Some(filename) = parts.last() {
            if filename.starts_with("round-") && filename.ends_with(".md") {
                let rest = &filename[6..];
                if rest.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if path is a protected state file.
fn is_protected_state_file(path_lower: &str) -> bool {
    // Check for state.md in .humanize/rlcr/*/ or .humanize/pr-loop/*/
    if path_lower.contains(".humanize/rlcr/") || path_lower.contains(".humanize/pr-loop/") {
        if path_lower.ends_with("/state.md") {
            return true;
        }
    }
    false
}

/// Handle stop commands.
pub fn handle_stop(cmd: StopCommands) -> Result<()> {
    match cmd {
        StopCommands::Rlcr => {
            // TODO: Implement RLCR stop hook
            bail!("RLCR stop hook not yet implemented in Rust");
        }
        StopCommands::Pr => {
            // TODO: Implement PR loop stop hook
            bail!("PR loop stop hook not yet implemented in Rust");
        }
    }
}

/// Handle ask-codex command.
pub fn handle_ask_codex(prompt: &str, model: &str, effort: &str, timeout: u64) -> Result<()> {
    // For now, call out to the shell script
    let plugin_root = std::env::var("CLAUDE_PLUGIN_ROOT")
        .unwrap_or_else(|_| "/home/cupnfish/.claude/plugins/cache/humania/humanize/1.15.0".to_string());

    let ask_script = format!("{}/humanize/scripts/ask-codex.sh", plugin_root);

    let status = std::process::Command::new(&ask_script)
        .arg(prompt)
        .arg("--model")
        .arg(model)
        .arg("--effort")
        .arg(effort)
        .arg("--timeout")
        .arg(timeout.to_string())
        .status()?;

    if !status.success() {
        bail!("ask-codex failed with exit code: {:?}", status.code());
    }

    Ok(())
}

/// Handle gen-plan command.
pub fn handle_gen_plan(input: &str, output: &str) -> Result<()> {
    // For now, call out to the shell script
    let plugin_root = std::env::var("CLAUDE_PLUGIN_ROOT")
        .unwrap_or_else(|_| "/home/cupnfish/.claude/plugins/cache/humania/humanize/1.15.0".to_string());

    let validate_script = format!("{}/humanize/scripts/validate-gen-plan-io.sh", plugin_root);

    let status = std::process::Command::new(&validate_script)
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(output)
        .status()?;

    if !status.success() {
        bail!("gen-plan validation failed with exit code: {:?}", status.code());
    }

    Ok(())
}
