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

            let setup_script = format!("{}/scripts/setup-rlcr-loop.sh", plugin_root);

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

            let cancel_script = format!("{}/scripts/cancel-rlcr-loop.sh", plugin_root);

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
            // SECURITY: Fail closed on malformed input - block the operation
            eprintln!("Error: Could not parse hook input: {}", e);
            HookOutput::block(format!("Hook input validation failed: {}", e)).print();
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
///
/// Implements full parity with loop-read-validator.sh:
/// 1. Blocks todos files unless allowlisted
/// 2. For summary/prompt files: validates location, round number, and directory
fn validate_read(input: &HookInput) -> HookOutput {
    let file_path = match get_file_path(input) {
        Some(p) => p,
        None => return HookOutput::allow(), // No file_path, allow
    };

    let path_lower = file_path.to_lowercase();

    // Check for todos files - block unless allowlisted
    if humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Todos) {
        // Try to find active loop and check allowlist
        let project_root = match std::env::var("CLAUDE_PROJECT_DIR") {
            Ok(p) => p,
            Err(_) => return HookOutput::block(format!(
                "Reading todos files is not allowed: {}",
                file_path
            )),
        };

        let loop_base_dir = format!("{}/.humanize/rlcr", project_root);
        let session_id = input.session_id.as_deref();

        if let Some(loop_dir) = humanize_core::state::find_active_loop(
            std::path::Path::new(&loop_base_dir),
            session_id,
        ) {
            // Parse state to get current round
            let state_file = humanize_core::state::resolve_active_state_file(&loop_dir);
            if let Some(state_path) = state_file {
                if let Ok(content) = std::fs::read_to_string(&state_path) {
                    if let Ok(state) = humanize_core::state::State::from_markdown_strict(&content) {
                        if humanize_core::fs::is_allowlisted_file(&file_path, &loop_dir, state.current_round) {
                            return HookOutput::allow();
                        }
                    }
                }
            }
        }

        return HookOutput::block(format!(
            "Reading todos files is not allowed. Use native Task tools instead: {}",
            file_path
        ));
    }

    // Check for summary/prompt files
    let is_summary = humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Summary);
    let is_prompt = humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Prompt);

    if !is_summary && !is_prompt {
        return HookOutput::allow(); // Not a round file, allow
    }

    // This is a summary or prompt file - validate against active loop
    let project_root = match std::env::var("CLAUDE_PROJECT_DIR") {
        Ok(p) => p,
        Err(_) => return HookOutput::allow(), // No project context, allow
    };

    let loop_base_dir = format!("{}/.humanize/rlcr", project_root);
    let session_id = input.session_id.as_deref();

    let active_loop_dir = match humanize_core::state::find_active_loop(
        std::path::Path::new(&loop_base_dir),
        session_id,
    ) {
        Some(d) => d,
        None => return HookOutput::allow(), // No active loop, allow
    };

    // Get current round from state
    let state_file = match humanize_core::state::resolve_active_state_file(&active_loop_dir) {
        Some(s) => s,
        None => return HookOutput::allow(),
    };

    let state_content = match std::fs::read_to_string(&state_file) {
        Ok(c) => c,
        Err(_) => return HookOutput::allow(),
    };

    let state = match humanize_core::state::State::from_markdown_strict(&state_content) {
        Ok(s) => s,
        Err(_) => return HookOutput::block("Malformed state file, blocking operation for safety".to_string()),
    };

    let current_round = state.current_round;

    // Validate file location - must be in .humanize/rlcr/ or .humanize/pr-loop/
    if !humanize_core::fs::is_in_humanize_loop_dir(&path_lower) {
        let file_type = if is_summary { "summary" } else { "prompt" };
        return HookOutput::block(format!(
            "Reading {} is blocked. Read from the active loop: {}",
            file_path,
            active_loop_dir.display()
        ));
    }

    // Extract round number from filename
    let claude_round = match humanize_core::fs::extract_round_number(&file_path) {
        Some(r) => r,
        None => return HookOutput::allow(), // Can't determine round, allow
    };

    // Check if file is allowlisted
    if humanize_core::fs::is_allowlisted_file(&file_path, &active_loop_dir, current_round) {
        return HookOutput::allow();
    }

    // Validate round number
    if claude_round != current_round {
        let file_type = if is_summary { "summary" } else { "prompt" };
        return HookOutput::block(format!(
            "You tried to read round-{}-{}.md but current round is {}. Read from: {}",
            claude_round,
            file_type,
            current_round,
            active_loop_dir.display()
        ));
    }

    // Validate directory path - must match active loop directory
    let filename = file_path.rsplit('/').next().unwrap_or(&file_path);
    let correct_path = active_loop_dir.join(filename);

    if file_path != correct_path.to_string_lossy() {
        return HookOutput::block(format!(
            "You tried to read {} but the correct path is {}",
            file_path,
            correct_path.display()
        ));
    }

    HookOutput::allow()
}

/// Validate Write tool access.
///
/// Implements parity with loop-write-validator.sh:
/// 1. Block todos files
/// 2. Block prompt files (read-only)
/// 3. Block state.md and finalize-state.md
/// 4. Block goal-tracker after Round 0
/// 5. Validate summary file location and round number
fn validate_write(input: &HookInput) -> HookOutput {
    let file_path = match get_file_path(input) {
        Some(p) => p,
        None => return HookOutput::allow(),
    };

    let path_lower = file_path.to_lowercase();

    // Block todos files
    if humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Todos) {
        return HookOutput::block(format!(
            "Writing to todos files is not allowed. Use native Task tools instead: {}",
            file_path
        ));
    }

    // Block prompt files (read-only, generated by Codex)
    if humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Prompt) {
        return HookOutput::block(format!(
            "Writing to prompt files is not allowed. Prompts are generated by Codex: {}",
            file_path
        ));
    }

    // Check for protected state files (state.md and finalize-state.md)
    if is_protected_state_file(&path_lower) || is_finalize_state_file(&path_lower) {
        return HookOutput::block(format!(
            "Writing to protected state files is not allowed: {}",
            file_path
        ));
    }

    // Check if this is a summary file or goal-tracker that needs loop-aware validation
    let is_summary = humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Summary);
    let is_finalize_summary = path_lower.ends_with("finalize-summary.md");
    let is_goal_tracker = path_lower.ends_with("goal-tracker.md");
    let in_humanize_loop_dir = humanize_core::fs::is_in_humanize_loop_dir(&path_lower);

    if !is_summary && !is_finalize_summary && !is_goal_tracker && !in_humanize_loop_dir {
        return HookOutput::allow(); // Not a file we need to validate
    }

    // Get active loop for validation
    let project_root = match std::env::var("CLAUDE_PROJECT_DIR") {
        Ok(p) => p,
        Err(_) => return HookOutput::allow(),
    };

    let loop_base_dir = format!("{}/.humanize/rlcr", project_root);
    let session_id = input.session_id.as_deref();

    let active_loop_dir = match humanize_core::state::find_active_loop(
        std::path::Path::new(&loop_base_dir),
        session_id,
    ) {
        Some(d) => d,
        None => return HookOutput::allow(), // No active loop, allow
    };

    // Get current round from state
    let state_file = match humanize_core::state::resolve_active_state_file(&active_loop_dir) {
        Some(s) => s,
        None => return HookOutput::allow(),
    };

    let state_content = match std::fs::read_to_string(&state_file) {
        Ok(c) => c,
        Err(_) => return HookOutput::allow(),
    };

    let state = match humanize_core::state::State::from_markdown_strict(&state_content) {
        Ok(s) => s,
        Err(_) => return HookOutput::block("Malformed state file, blocking operation for safety".to_string()),
    };

    let current_round = state.current_round;

    // Block goal-tracker after Round 0
    if is_goal_tracker && current_round > 0 {
        let summary_file = format!("{}/round-{}-summary.md", active_loop_dir.display(), current_round);
        return HookOutput::block(format!(
            "Writing to goal-tracker.md is not allowed after Round 0. Update it in your summary: {}",
            summary_file
        ));
    }

    // Allow finalize-summary.md in finalize phase
    if is_finalize_summary && in_humanize_loop_dir {
        let expected_path = format!("{}/finalize-summary.md", active_loop_dir.display());
        if file_path == expected_path {
            return HookOutput::allow();
        }
    }

    // Block summary files outside .humanize/rlcr/
    if is_summary && !in_humanize_loop_dir {
        let correct_path = format!("{}/round-{}-summary.md", active_loop_dir.display(), current_round);
        return HookOutput::block(format!(
            "Write summary to the correct path: {}",
            correct_path
        ));
    }

    // Validate round number for summary files
    if is_summary {
        if let Some(claude_round) = humanize_core::fs::extract_round_number(&file_path) {
            if claude_round != current_round {
                let correct_path = format!("{}/round-{}-summary.md", active_loop_dir.display(), current_round);
                return HookOutput::block(format!(
                    "You tried to write to round-{}-summary.md but current round is {}. Write to: {}",
                    claude_round, current_round, correct_path
                ));
            }
        }
    }

    // Validate directory path for files in .humanize/rlcr/
    if in_humanize_loop_dir {
        let filename = file_path.rsplit('/').next().unwrap_or(&file_path);
        let correct_path = format!("{}/{}", active_loop_dir.display(), filename);
        if file_path != correct_path {
            return HookOutput::block(format!(
                "You tried to write to {} but the correct path is {}",
                file_path, correct_path
            ));
        }
    }

    HookOutput::allow()
}

/// Check if path is a finalize-state.md file.
fn is_finalize_state_file(path_lower: &str) -> bool {
    path_lower.ends_with("/finalize-state.md")
}

/// Validate Edit tool access.
///
/// Implements parity with loop-edit-validator.sh:
/// 1. Block todos files
/// 2. Block prompt files (read-only)
/// 3. Block state.md and finalize-state.md
/// 4. Block goal-tracker after Round 0
/// 5. Validate summary file round number
fn validate_edit(input: &HookInput) -> HookOutput {
    let file_path = match get_file_path(input) {
        Some(p) => p,
        None => return HookOutput::allow(),
    };

    let path_lower = file_path.to_lowercase();

    // Block todos files
    if humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Todos) {
        return HookOutput::block(format!(
            "Editing todos files is not allowed. Use native Task tools instead: {}",
            file_path
        ));
    }

    // Block prompt files (read-only, generated by Codex)
    if humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Prompt) {
        return HookOutput::block(format!(
            "Editing prompt files is not allowed. Prompts are generated by Codex: {}",
            file_path
        ));
    }

    // Check for protected state files (state.md and finalize-state.md)
    if is_finalize_state_file(&path_lower) {
        return HookOutput::block(format!(
            "Editing finalize-state.md is not allowed: {}",
            file_path
        ));
    }

    if is_protected_state_file(&path_lower) {
        return HookOutput::block(format!(
            "Editing state.md is not allowed: {}",
            file_path
        ));
    }

    // Check if file is in .humanize/rlcr/ - if not, allow
    if !humanize_core::fs::is_in_humanize_loop_dir(&path_lower) {
        return HookOutput::allow();
    }

    // Get active loop for validation
    let project_root = match std::env::var("CLAUDE_PROJECT_DIR") {
        Ok(p) => p,
        Err(_) => return HookOutput::allow(),
    };

    let loop_base_dir = format!("{}/.humanize/rlcr", project_root);
    let session_id = input.session_id.as_deref();

    let active_loop_dir = match humanize_core::state::find_active_loop(
        std::path::Path::new(&loop_base_dir),
        session_id,
    ) {
        Some(d) => d,
        None => return HookOutput::allow(), // No active loop, allow
    };

    // Get current round from state
    let state_file = match humanize_core::state::resolve_active_state_file(&active_loop_dir) {
        Some(s) => s,
        None => return HookOutput::allow(),
    };

    let state_content = match std::fs::read_to_string(&state_file) {
        Ok(c) => c,
        Err(_) => return HookOutput::allow(),
    };

    let state = match humanize_core::state::State::from_markdown_strict(&state_content) {
        Ok(s) => s,
        Err(_) => return HookOutput::block("Malformed state file, blocking operation for safety".to_string()),
    };

    let current_round = state.current_round;

    // Block plan.md backup edits
    let filename = file_path.rsplit('/').next().unwrap_or(&file_path);
    if filename == "plan.md" {
        return HookOutput::block(format!(
            "Editing plan.md backup is not allowed during RLCR loop: {}",
            file_path
        ));
    }

    // Block goal-tracker after Round 0
    if path_lower.ends_with("goal-tracker.md") && current_round > 0 {
        let summary_file = format!("{}/round-{}-summary.md", active_loop_dir.display(), current_round);
        return HookOutput::block(format!(
            "Editing goal-tracker.md is not allowed after Round 0. Update it in your summary: {}",
            summary_file
        ));
    }

    // Validate round number for summary files
    if humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Summary) {
        if let Some(claude_round) = humanize_core::fs::extract_round_number(&file_path) {
            if claude_round != current_round {
                let correct_path = format!("{}/round-{}-summary.md", active_loop_dir.display(), current_round);
                return HookOutput::block(format!(
                    "You tried to edit round-{}-summary.md but current round is {}. Edit: {}",
                    claude_round, current_round, correct_path
                ));
            }
        }
    }

    HookOutput::allow()
}

/// Validate Bash command execution.
///
/// Implements parity with loop-bash-validator.sh:
/// 1. Block git add targeting .humanize
/// 2. Block file redirections to protected files
/// 3. Block sed -i for protected files
/// 4. Block dangerous shell patterns
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

    // Block git add targeting .humanize
    if cmd_lower.starts_with("git add") || cmd_lower.starts_with("git add") {
        if cmd_lower.contains(".humanize") {
            return HookOutput::block(format!(
                "Adding .humanize files to git is not allowed: {}",
                cmd_trimmed
            ));
        }
    }

    // Block file redirections to protected files
    let protected_patterns = [
        "state.md", "finalize-state.md", "goal-tracker.md",
        "round-", "-summary.md", "-prompt.md", "-todos.md",
    ];

    // Check for > and >> redirections
    if cmd_lower.contains("> ") || cmd_lower.contains(">>") {
        for protected in &protected_patterns {
            if cmd_lower.contains(protected) {
                return HookOutput::block(format!(
                    "Cannot redirect output to protected file '{}': {}",
                    protected, cmd_trimmed
                ));
            }
        }
    }

    // Block sed -i (in-place edit) for protected files
    if cmd_lower.contains("sed") && cmd_lower.contains("-i") {
        for protected in &protected_patterns {
            if cmd_lower.contains(protected) {
                return HookOutput::block(format!(
                    "Cannot edit protected file '{}' in-place: {}",
                    protected, cmd_trimmed
                ));
            }
        }
    }

    // Block tee for protected files
    if cmd_lower.contains("tee ") {
        for protected in &protected_patterns {
            if cmd_lower.contains(protected) {
                return HookOutput::block(format!(
                    "Cannot tee to protected file '{}': {}",
                    protected, cmd_trimmed
                ));
            }
        }
    }

    // Dangerous patterns
    let dangerous_patterns = [
        "rm ", "rm\t", "rmdir", "mv ", "mv\t", "cp ", "2>",
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
///
/// This implements the session handshake from loop-post-bash-hook.sh:
/// 1. Reads .pending-session-id signal file (2 lines: state path, command signature)
/// 2. Verifies bash command starts with command signature (boundary-aware match)
/// 3. Extracts session_id from hook input
/// 4. Patches state.md by replacing empty session_id with actual value
/// 5. Removes signal file (one-shot mechanism)
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
        // Malformed signal file - clean up and exit
        let _ = std::fs::remove_file(&pending_file);
        return HookOutput::allow();
    }

    let state_file_path = lines[0];
    let command_signature = lines[1];

    // Validate state file exists
    if state_file_path.is_empty() || !std::path::Path::new(state_file_path).exists() {
        let _ = std::fs::remove_file(&pending_file);
        return HookOutput::allow();
    }

    // Get the bash command from tool input
    let command = match get_command(input) {
        Some(c) => c,
        None => return HookOutput::allow(),
    };

    // Boundary-aware match: command must start with signature followed by
    // end-of-string or whitespace (prevents substring false positives)
    let is_setup_invocation = {
        // Check quoted form: "signature" or "signature" followed by space/tab
        let quoted = format!("\"{}\"", command_signature);
        command == quoted || command.starts_with(&format!("{} ", quoted)) || command.starts_with(&format!("{}\t", quoted))
            ||
        // Check unquoted form: signature or signature followed by space/tab
        command == command_signature || command.starts_with(&format!("{} ", command_signature)) || command.starts_with(&format!("{}\t", command_signature))
    };

    if !is_setup_invocation {
        // This bash event is not from the setup script - don't consume signal
        return HookOutput::allow();
    }

    // Get session_id from hook input
    let session_id = match &input.session_id {
        Some(s) if !s.is_empty() => s.clone(),
        _ => return HookOutput::allow(), // No session_id available, leave signal for next attempt
    };

    // Read current state file
    let state_content = match std::fs::read_to_string(state_file_path) {
        Ok(c) => c,
        Err(_) => {
            let _ = std::fs::remove_file(&pending_file);
            return HookOutput::allow();
        }
    };

    // Check if session_id is currently empty (safety check)
    let has_empty_session_id = state_content
        .lines()
        .any(|line| line == "session_id:" || line == "session_id: " || line == "session_id: ~" || line == "session_id: null");

    if has_empty_session_id {
        // Patch state.md by replacing empty session_id with actual value
        let patched = state_content
            .lines()
            .map(|line| {
                if line == "session_id:" || line == "session_id: " || line == "session_id: ~" || line == "session_id: null" {
                    format!("session_id: {}", session_id)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Write patched content atomically
        let temp_path = format!("{}.tmp.{}", state_file_path, std::process::id());
        if std::fs::write(&temp_path, patched).is_ok() {
            if std::fs::rename(&temp_path, state_file_path).is_err() {
                let _ = std::fs::remove_file(&temp_path);
            }
        }
    }

    // Remove signal file (one-shot: session_id is now recorded)
    let _ = std::fs::remove_file(&pending_file);

    HookOutput::allow()
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

    let ask_script = format!("{}/scripts/ask-codex.sh", plugin_root);

    // Shell expects --codex-model MODEL:EFFORT and --codex-timeout SECONDS
    let model_effort = format!("{}:{}", model, effort);

    let status = std::process::Command::new(&ask_script)
        .arg(prompt)
        .arg("--codex-model")
        .arg(&model_effort)
        .arg("--codex-timeout")
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

    let validate_script = format!("{}/scripts/validate-gen-plan-io.sh", plugin_root);

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
