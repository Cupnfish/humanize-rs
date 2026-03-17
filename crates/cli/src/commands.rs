//! Command handlers for the Humanize CLI.

use anyhow::{bail, Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Terminal;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::hook_input::{get_command, get_file_path, read_hook_input, HookInput, HookOutput};
use crate::{CancelCommands, GateCommands, HookCommands, MonitorCommands, SetupCommands, StopCommands};

/// Handle setup commands.
pub fn handle_setup(cmd: SetupCommands) -> Result<()> {
    match cmd {
        SetupCommands::Rlcr {
            plan_file,
            plan_file_explicit,
            track_plan_file,
            max_iterations,
            base_branch,
            codex_model,
            codex_timeout,
            push_every_round,
            full_review_round,
            skip_impl,
            claude_answer_codex,
            agent_teams,
        } => setup_rlcr_native(SetupRlcrOptions {
            positional_plan_file: plan_file,
            explicit_plan_file: plan_file_explicit,
            track_plan_file,
            max_iterations,
            base_branch,
            codex_model,
            codex_timeout,
            push_every_round,
            full_review_round,
            skip_impl,
            ask_codex_question: !claude_answer_codex,
            agent_teams,
        }),
        SetupCommands::Pr {
            claude,
            codex,
            max_iterations,
            codex_model,
            codex_timeout,
        } => setup_pr_native(SetupPrOptions {
            claude,
            codex,
            max_iterations,
            codex_model,
            codex_timeout,
        }),
    }
}

/// Handle cancel commands.
pub fn handle_cancel(cmd: CancelCommands) -> Result<()> {
    match cmd {
        CancelCommands::Rlcr { force } => cancel_rlcr_native(force),
        CancelCommands::Pr => {
            cancel_pr_native()
        }
    }
}

/// Handle hook commands - all read JSON from stdin.
pub fn handle_hook(cmd: HookCommands) -> Result<()> {
    // Read hook input from stdin
    let require_tool_name = !matches!(cmd, HookCommands::PlanFileValidator);
    let input = match read_hook_input(require_tool_name) {
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
        return HookOutput::block(render_template_or_fallback(
            "block/prompt-file-write.md",
            "# Prompt File Write Blocked\n\nYou cannot write to `round-*-prompt.md` files.",
            &[],
        ));
    }

    // Check for protected state files (state.md and finalize-state.md)
    if is_protected_state_file(&path_lower) || is_finalize_state_file(&path_lower) {
        let template_name = if path_lower.contains(".humanize/pr-loop/") {
            "block/pr-loop-state-modification.md"
        } else {
            "block/state-file-modification.md"
        };
        return HookOutput::block(render_template_or_fallback(
            template_name,
            "# State File Modification Blocked\n\nYou cannot modify `state.md`.",
            &[],
        ));
    }

    if is_pr_loop_readonly_file(&path_lower) {
        return HookOutput::block(pr_loop_readonly_reason());
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

fn is_pr_loop_readonly_file(path_lower: &str) -> bool {
    path_lower.contains(".humanize/pr-loop/")
        && (path_lower.ends_with("-pr-comment.md")
            || path_lower.ends_with("-pr-check.md")
            || path_lower.ends_with("-pr-feedback.md")
            || path_lower.ends_with("-codex-prompt.md"))
}

fn pr_loop_readonly_reason() -> String {
    render_template_or_fallback(
        "block/pr-loop-prompt-write.md",
        "# PR Loop File Write Blocked\n\nYou cannot write to `round-*-pr-comment.md`, `round-*-pr-check.md`, `round-*-pr-feedback.md`, or `round-*-codex-prompt.md` files in `.humanize/pr-loop/`.\nThese files are generated and managed by the PR loop system.",
        &[],
    )
}

/// Protected file patterns that should not be modified via shell commands.
const PROTECTED_PATTERNS: &[&str] = &[
    "state.md", "finalize-state.md", "goal-tracker.md",
    "round-", "-summary.md", "-prompt.md", "-todos.md",
    "-pr-comment.md", "-pr-check.md", "-pr-feedback.md", "-codex-prompt.md",
];

/// Check if a command contains any protected file pattern.
fn contains_protected_pattern(cmd_lower: &str) -> Option<&'static str> {
    PROTECTED_PATTERNS.iter().find(|p| cmd_lower.contains(*p)).copied()
}

fn bash_has_control_operators(cmd_lower: &str) -> bool {
    [
        "|", "&&", ";", "`", "$(", "||", "<(", ">(", "\n",
    ]
    .iter()
    .any(|pattern| cmd_lower.contains(pattern))
        || cmd_lower.trim_end().ends_with('&')
}

fn git_adds_humanize(cmd_trimmed: &str) -> bool {
    let cmd_lower = cmd_trimmed.to_lowercase();
    if !cmd_lower.starts_with("git add") {
        return false;
    }
    if cmd_lower.contains(".humanize") {
        return true;
    }

    let tokens = cmd_trimmed.split_whitespace().collect::<Vec<_>>();
    for token in tokens.iter().skip(2) {
        let lower = token.to_ascii_lowercase();
        if lower == "." || lower == "*" || lower == "./" || lower == ":/" {
            return true;
        }
        if lower.starts_with('-')
            && (lower.contains('a')
                || lower.contains('u')
                || lower == "--all"
                || lower == "--update")
        {
            return true;
        }
    }

    false
}

fn command_modifies_protected_file(cmd_lower: &str) -> bool {
    let Some(_) = contains_protected_pattern(cmd_lower) else {
        return false;
    };

    let write_like_patterns = [
        ">", ">>", "tee ", "sed -i", "perl -i", "ruby -i", "truncate ", "dd ", "install ",
        "mv ", "cp ", "touch ", "rm ", "python -c", "python3 -c", "node -e", "ed ", "ex ",
        "exec >", "xargs ",
    ];

    write_like_patterns
        .iter()
        .any(|pattern| cmd_lower.contains(pattern))
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
        return HookOutput::block(render_template_or_fallback(
            "block/prompt-file-write.md",
            "# Prompt File Write Blocked\n\nYou cannot write to `round-*-prompt.md` files.",
            &[],
        ));
    }

    // Check for protected state files (state.md and finalize-state.md)
    if is_finalize_state_file(&path_lower) {
        return HookOutput::block(render_template_or_fallback(
            "block/finalize-state-file-modification.md",
            "# Finalize State File Modification Blocked\n\nYou cannot modify `finalize-state.md`.",
            &[],
        ));
    }

    if is_protected_state_file(&path_lower) {
        let template_name = if path_lower.contains(".humanize/pr-loop/") {
            "block/pr-loop-state-modification.md"
        } else {
            "block/state-file-modification.md"
        };
        return HookOutput::block(render_template_or_fallback(
            template_name,
            "# State File Modification Blocked\n\nYou cannot modify `state.md`.",
            &[],
        ));
    }

    if is_pr_loop_readonly_file(&path_lower) {
        return HookOutput::block(pr_loop_readonly_reason());
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
        "ls ", "cat ", "head ", "tail ", "grep ", "which ", "pwd",
        "rg ", "find ", "wc ", "sed -n", "git show", "git remote", "git ls-files",
    ];

    for pattern in &safe_patterns {
        if cmd_lower.starts_with(&pattern.to_lowercase()) && !bash_has_control_operators(&cmd_lower) {
            return HookOutput::allow();
        }
    }

    // Block git add targeting .humanize or broad add patterns
    if git_adds_humanize(cmd_trimmed) {
        return HookOutput::block(format!(
            "Adding .humanize files to git, or using broad `git add` patterns during an active loop, is not allowed: {}",
            cmd_trimmed
        ));
    }

    if command_modifies_protected_file(&cmd_lower) {
        if let Some(protected) = contains_protected_pattern(&cmd_lower) {
            let reason = if protected.contains("summary") {
                render_template_or_fallback(
                    "block/summary-bash-write.md",
                    "# Bash Write Blocked: Use Write or Edit Tool\n\nDo not use Bash commands to modify summary files.\n\nUse the Write or Edit tool instead: {{CORRECT_PATH}}",
                    &[("CORRECT_PATH", "the current round summary file".to_string())],
                )
            } else if protected.contains("prompt") {
                render_template_or_fallback(
                    "block/prompt-file-write.md",
                    "# Prompt File Write Blocked\n\nYou cannot write to prompt files.",
                    &[],
                )
            } else if protected.contains("state") {
                render_template_or_fallback(
                    "block/state-file-modification.md",
                    "# State File Modification Blocked\n\nYou cannot modify `state.md`.",
                    &[],
                )
            } else {
                format!(
                    "Cannot modify protected loop file '{}' via Bash: {}",
                    protected, cmd_trimmed
                )
            };
            return HookOutput::block(reason);
        }
    }

    // Dangerous patterns
    let dangerous_patterns = [
        "rm ", "rm\t", "rmdir", "mv ", "mv\t", "cp ", "2>",
        "chmod", "chown", "mkdir -p", "nohup", "disown", "bg ", "fg ",
    ];

    for pattern in &dangerous_patterns {
        if cmd_trimmed.contains(pattern) {
            return HookOutput::block(format!(
                "Command contains potentially dangerous pattern '{}': {}",
                pattern, cmd_trimmed
            ));
        }
    }

    if bash_has_control_operators(&cmd_lower) {
        return HookOutput::block(format!(
            "Command contains shell control operators that are not allowed in loop Bash validation: {}",
            cmd_trimmed
        ));
    }

    HookOutput::allow()
}

/// Validate plan file path.
fn validate_plan_file(input: &HookInput) -> HookOutput {
    let project_root = std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()));

    let project_root = match project_root {
        Some(p) => p,
        None => return HookOutput::block(
            "Could not determine project root. Please set CLAUDE_PROJECT_DIR and try again.",
        ),
    };

    let loop_base_dir = Path::new(&project_root).join(".humanize/rlcr");
    let active_loop_dir =
        match humanize_core::state::find_active_loop(&loop_base_dir, input.session_id.as_deref()) {
            Some(d) => d,
            None => return HookOutput::allow(),
        };

    let state_file = match humanize_core::state::resolve_active_state_file(&active_loop_dir) {
        Some(s) => s,
        None => return HookOutput::allow(),
    };

    let state_content = match std::fs::read_to_string(&state_file) {
        Ok(c) => c,
        Err(_) => {
            return HookOutput::block("Malformed state file, blocking operation for safety")
        }
    };

    let state = match humanize_core::state::State::from_markdown_strict(&state_content) {
        Ok(s) => s,
        Err(_) => {
            return HookOutput::block("Malformed state file, blocking operation for safety")
        }
    };

    if let Some(reason) = validate_plan_state_schema(&state_content) {
        return HookOutput::block(reason);
    }

    let current_branch = match git_current_branch(Path::new(&project_root)) {
        Ok(branch) => branch,
        Err(_) => {
            return HookOutput::block(
                "Git operation failed or timed out.\n\nCannot verify branch consistency. Please check git status and try again.",
            )
        }
    };

    if !state.start_branch.trim().is_empty() && current_branch != state.start_branch {
        return HookOutput::block(format!(
            "Git branch has changed during RLCR loop.\n\nStarted on: {}\nCurrent: {}\n\nBranch switching is not allowed during an active RLCR loop. Please switch back to the original branch or cancel the loop with /humanize:cancel-rlcr-loop",
            state.start_branch, current_branch
        ));
    }

    let plan_is_tracked = match git_path_is_tracked(Path::new(&project_root), &state.plan_file) {
        Ok(v) => v,
        Err(GitPathCheckError::Failed(code)) => {
            return HookOutput::block(format!(
                "Git operation failed while checking plan file tracking status (exit code: {}).\n\nPlease check git status and try again.",
                code
            ))
        }
        Err(GitPathCheckError::Io(err)) => {
            return HookOutput::block(format!(
                "Git operation failed while checking plan file tracking status.\n\n{}\n\nPlease check git status and try again.",
                err
            ))
        }
    };

    if state.plan_tracked {
        if !plan_is_tracked {
            return HookOutput::block(format!(
                "Plan file is no longer tracked in git.\n\nFile: {}\n\nThis RLCR loop was started with --track-plan-file, but the plan file has been removed from git tracking.",
                state.plan_file
            ));
        }

        let plan_git_status = match git_path_status_porcelain(Path::new(&project_root), &state.plan_file) {
            Ok(status) => status,
            Err(GitPathCheckError::Failed(code)) => {
                return HookOutput::block(format!(
                    "Git operation failed while checking plan file status (exit code: {}).\n\nPlease check git status and try again.",
                    code
                ))
            }
            Err(GitPathCheckError::Io(err)) => {
                return HookOutput::block(format!(
                    "Git operation failed while checking plan file status.\n\n{}\n\nPlease check git status and try again.",
                    err
                ))
            }
        };

        if !plan_git_status.trim().is_empty() {
            return HookOutput::block(format!(
                "Plan file has uncommitted modifications.\n\nFile: {}\nStatus: {}\n\nThis RLCR loop was started with --track-plan-file. Plan file modifications are not allowed during the loop.",
                state.plan_file,
                plan_git_status.trim()
            ));
        }
    } else if plan_is_tracked {
        return HookOutput::block(format!(
            "Plan file is now tracked in git but loop was started without --track-plan-file.\n\nFile: {}\n\nThe plan file must remain gitignored during this RLCR loop.",
            state.plan_file
        ));
    }

    HookOutput::allow()
}

fn validate_plan_state_schema(state_content: &str) -> Option<String> {
    let mapping = match extract_state_yaml_mapping(state_content) {
        Ok(m) => m,
        Err(_) => {
            return Some("Malformed state file, blocking operation for safety".to_string())
        }
    };

    if !mapping.contains_key(serde_yaml::Value::String("plan_tracked".to_string())) {
        return Some(outdated_schema_reason("plan_tracked"));
    }

    match mapping.get(serde_yaml::Value::String("start_branch".to_string())) {
        Some(serde_yaml::Value::String(value)) if !value.trim().is_empty() => None,
        Some(_) => Some(outdated_schema_reason("start_branch")),
        None => Some(outdated_schema_reason("start_branch")),
    }
}

fn outdated_schema_reason(field_name: &str) -> String {
    format!(
        "RLCR loop state file is missing required field: `{}`\n\nThis indicates the loop was started with an older version of humanize.\n\nOptions:\n1. Cancel the loop: `/humanize:cancel-rlcr-loop`\n2. Update humanize and restart the loop\n3. Restart the RLCR loop with the updated plugin",
        field_name
    )
}

fn extract_state_yaml_mapping(state_content: &str) -> std::result::Result<serde_yaml::Mapping, ()> {
    let content = state_content.trim();
    if !content.starts_with("---") {
        return Err(());
    }

    let rest = &content[3..];
    let end_pos = rest.find("\n---").ok_or(())?;
    let yaml_content = &rest[..end_pos];
    let yaml_value: serde_yaml::Value = serde_yaml::from_str(yaml_content).map_err(|_| ())?;
    yaml_value.as_mapping().cloned().ok_or(())
}

#[derive(Debug)]
enum GitPathCheckError {
    Io(std::io::Error),
    Failed(i32),
}

fn git_current_branch(repo_path: &Path) -> std::result::Result<String, GitPathCheckError> {
    let output = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or(".")])
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(GitPathCheckError::Io)?;

    if !output.status.success() {
        return Err(GitPathCheckError::Failed(
            output.status.code().unwrap_or(1),
        ));
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        return Err(GitPathCheckError::Failed(1));
    }

    Ok(branch)
}

fn git_path_is_tracked(repo_path: &Path, path: &str) -> std::result::Result<bool, GitPathCheckError> {
    let output = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or(".")])
        .args(["ls-files", "--error-unmatch", path])
        .output()
        .map_err(GitPathCheckError::Io)?;

    match output.status.code().unwrap_or(1) {
        0 => Ok(true),
        1 => Ok(false),
        code => Err(GitPathCheckError::Failed(code)),
    }
}

fn git_path_status_porcelain(
    repo_path: &Path,
    path: &str,
) -> std::result::Result<String, GitPathCheckError> {
    let output = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or(".")])
        .args(["status", "--porcelain", path])
        .output()
        .map_err(GitPathCheckError::Io)?;

    if !output.status.success() {
        return Err(GitPathCheckError::Failed(
            output.status.code().unwrap_or(1),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[derive(Debug, Clone)]
struct SetupRlcrOptions {
    positional_plan_file: Option<String>,
    explicit_plan_file: Option<String>,
    track_plan_file: bool,
    max_iterations: u32,
    base_branch: Option<String>,
    codex_model: String,
    codex_timeout: u64,
    push_every_round: bool,
    full_review_round: u32,
    skip_impl: bool,
    ask_codex_question: bool,
    agent_teams: bool,
}

#[derive(Debug, Clone)]
struct SetupPrOptions {
    claude: bool,
    codex: bool,
    max_iterations: u32,
    codex_model: String,
    codex_timeout: u64,
}

fn setup_rlcr_native(options: SetupRlcrOptions) -> Result<()> {
    let project_root = resolve_project_root()?;
    let plugin_root = resolve_plugin_root()?;

    let chosen_plan = match (&options.positional_plan_file, &options.explicit_plan_file) {
        (Some(_), Some(_)) => bail!("Error: cannot specify both positional plan file and --plan-file"),
        (Some(path), None) => Some(path.clone()),
        (None, Some(path)) => Some(path.clone()),
        (None, None) if options.skip_impl => None,
        (None, None) => bail!("Error: missing plan file"),
    };

    let clean = humanize_core::git::is_working_tree_clean(&project_root)
        .context("Error: failed to check git working tree status")?;
    if !clean {
        bail!(
            "Error: Git working tree is not clean\n\nRLCR loop can only be started on a clean git repository.\nPlease commit or stash your changes before starting the loop."
        );
    }

    let start_branch =
        humanize_core::git::get_current_branch(&project_root).context("Error: failed to get current branch")?;

    let (codex_model, codex_effort) = parse_model_and_effort(
        &options.codex_model,
        humanize_core::constants::DEFAULT_CODEX_EFFORT,
    );

    let loop_base_dir = project_root.join(".humanize/rlcr");
    fs::create_dir_all(&loop_base_dir)?;
    let timestamp = loop_timestamp();
    let loop_dir = loop_base_dir.join(&timestamp);
    fs::create_dir_all(&loop_dir)?;

    let skip_impl_no_plan = chosen_plan.is_none();
    let mut line_count = 0usize;

    let state_plan_file = if let Some(plan_file) = chosen_plan.clone() {
        let validation = validate_setup_plan_file(&project_root, &plan_file, options.track_plan_file)?;
        line_count = validation.line_count;
        create_plan_backup(&validation.full_path, &loop_dir.join("plan.md"))?;
        plan_file
    } else {
        let placeholder = loop_dir.join("plan.md");
        fs::write(&placeholder, skip_impl_plan_placeholder())?;
        format!(".humanize/rlcr/{}/plan.md", timestamp)
    };

    let base_branch = detect_base_branch(&project_root, options.base_branch.as_deref())?;
    let base_commit = git_rev_parse(&project_root, &base_branch)?;

    let state = humanize_core::state::State::new_rlcr(
        state_plan_file.clone(),
        if skip_impl_no_plan { false } else { options.track_plan_file },
        start_branch.clone(),
        base_branch.clone(),
        base_commit.clone(),
        Some(options.max_iterations),
        Some(codex_model.clone()),
        Some(codex_effort.clone()),
        Some(options.codex_timeout),
        options.push_every_round,
        Some(options.full_review_round),
        options.ask_codex_question,
        options.agent_teams,
        options.skip_impl,
    );
    state.save(loop_dir.join("state.md"))?;

    fs::create_dir_all(project_root.join(".humanize"))?;
    let pending_session = project_root.join(".humanize/.pending-session-id");
    let command_signature = plugin_root.join("scripts/setup-rlcr-loop.sh");
    fs::write(
        &pending_session,
        format!(
            "{}\n{}\n",
            loop_dir.join("state.md").display(),
            command_signature.display()
        ),
    )?;

    if options.skip_impl {
        fs::write(loop_dir.join(".review-phase-started"), "build_finish_round=0\n")?;
    }

    let goal_tracker_path = loop_dir.join("goal-tracker.md");
    let summary_path = loop_dir.join("round-0-summary.md");
    let prompt_path = loop_dir.join("round-0-prompt.md");

    if options.skip_impl {
        fs::write(&goal_tracker_path, skip_impl_goal_tracker())?;
        fs::write(
            &prompt_path,
            skip_impl_round_prompt(
                &summary_path,
                &base_branch,
                &start_branch,
            ),
        )?;
    } else {
        let plan_full_path = project_root.join(&state_plan_file);
        let plan_content = fs::read_to_string(&plan_full_path)?;
        fs::write(
            &goal_tracker_path,
            build_goal_tracker(&plan_content, &state_plan_file),
        )?;
        fs::write(
            &prompt_path,
            build_round_0_prompt(
                &loop_dir.join("plan.md"),
                &goal_tracker_path,
                &summary_path,
                options.push_every_round,
                options.agent_teams,
            )?,
        )?;
    }

    print_setup_banner(
        &state_plan_file,
        line_count,
        &start_branch,
        &base_branch,
        options.max_iterations,
        &codex_model,
        &codex_effort,
        options.codex_timeout,
        options.full_review_round,
        options.ask_codex_question,
        &loop_dir,
        options.skip_impl,
    )?;

    print!("{}", fs::read_to_string(&prompt_path)?);
    Ok(())
}

fn setup_pr_native(options: SetupPrOptions) -> Result<()> {
    if !options.claude && !options.codex {
        bail!("Error: At least one bot flag is required (--claude or --codex)");
    }

    let project_root = resolve_project_root()?;
    if humanize_core::state::find_active_loop(&project_root.join(".humanize/rlcr"), None).is_some() {
        bail!("Error: An RLCR loop is already active");
    }
    if newest_active_pr_loop(&project_root.join(".humanize/pr-loop")).is_some() {
        bail!("Error: A PR loop is already active");
    }

    ensure_command_exists("gh", "Error: start-pr-loop requires the GitHub CLI (gh) to be installed")?;
    ensure_command_exists("codex", "Error: start-pr-loop requires codex to run")?;
    ensure_gh_auth(&project_root)?;

    let start_branch =
        humanize_core::git::get_current_branch(&project_root).context("Error: Failed to get current branch")?;
    let current_repo = gh_current_repo(&project_root)?;
    let pr_number = gh_detect_pr_number(&project_root)?;
    let pr_state = gh_pr_state(&project_root, pr_number)?;
    if pr_state == "MERGED" {
        bail!("Error: PR #{} has already been merged", pr_number);
    }
    if pr_state == "CLOSED" {
        bail!("Error: PR #{} has been closed", pr_number);
    }

    let commit_info = gh_pr_commit_info(&project_root, pr_number)?;
    let active_bots = build_active_bots(&options);
    let startup = gh_startup_case(&project_root, &current_repo, pr_number, &active_bots, &commit_info.latest_commit_at)?;

    let loop_base_dir = project_root.join(".humanize/pr-loop");
    fs::create_dir_all(&loop_base_dir)?;
    let timestamp = loop_timestamp();
    let loop_dir = loop_base_dir.join(&timestamp);
    fs::create_dir_all(&loop_dir)?;

    let comment_file = loop_dir.join("round-0-pr-comment.md");
    let comments_md = format_initial_pr_comments(&startup.comments);
    fs::write(&comment_file, comments_md)?;

    let mut trigger_comment_id = String::new();
    let mut last_trigger_at = String::new();
    if startup.case_num == 4 || startup.case_num == 5 {
        let mention = build_bot_mention_string(&active_bots);
        let body = format!(
            "{} please review the latest changes (new commits since last review)",
            mention
        );
        let comment_status = Command::new("gh")
            .args(["pr", "comment", &pr_number.to_string(), "--repo", &current_repo, "--body", &body])
            .current_dir(&project_root)
            .status()?;
        if !comment_status.success() {
            let _ = fs::remove_dir_all(&loop_dir);
            bail!("Error: Failed to post trigger comment");
        }

        if let Ok(user) = gh_current_user(&project_root) {
            if let Some((id, created_at)) =
                gh_find_latest_user_comment(&project_root, &current_repo, pr_number, &user)?
            {
                trigger_comment_id = id.to_string();
                last_trigger_at = created_at;
            }
        }

        if trigger_comment_id.is_empty() {
            let _ = fs::remove_dir_all(&loop_dir);
            bail!("Error: Could not find trigger comment ID for eyes verification");
        }
    }

    let (codex_model, codex_effort) = parse_model_and_effort(
        &options.codex_model,
        "medium",
    );

    let state = humanize_core::state::State {
        current_round: 0,
        max_iterations: options.max_iterations,
        codex_model: codex_model.clone(),
        codex_effort: codex_effort.clone(),
        codex_timeout: options.codex_timeout,
        start_branch: start_branch.clone(),
        pr_number: Some(pr_number),
        configured_bots: Some(active_bots.clone()),
        active_bots: Some(active_bots.clone()),
        poll_interval: Some(30),
        poll_timeout: Some(900),
        started_at: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        startup_case: Some(startup.case_num.to_string()),
        latest_commit_sha: Some(commit_info.latest_commit_sha.clone()),
        latest_commit_at: Some(commit_info.latest_commit_at.clone()),
        last_trigger_at: if last_trigger_at.is_empty() {
            None
        } else {
            Some(last_trigger_at.clone())
        },
        trigger_comment_id: if trigger_comment_id.is_empty() {
            None
        } else {
            Some(trigger_comment_id.clone())
        },
        ..humanize_core::state::State::default()
    };
    state.save(loop_dir.join("state.md"))?;

    let goal_tracker = build_pr_goal_tracker(
        pr_number,
        &start_branch,
        &active_bots,
        startup.case_num,
        state.started_at.as_deref().unwrap_or(""),
    );
    fs::write(loop_dir.join("goal-tracker.md"), goal_tracker)?;

    let prompt = build_pr_round_0_prompt(
        pr_number,
        &start_branch,
        &active_bots,
        &comment_file,
        &loop_dir.join("round-0-pr-resolve.md"),
        startup.comments.is_empty(),
    );
    fs::write(loop_dir.join("round-0-prompt.md"), &prompt)?;

    println!("=== start-pr-loop activated ===\n");
    println!("PR Number: #{}", pr_number);
    println!("Branch: {}", start_branch);
    println!("Active Bots: {}", active_bots.join(", "));
    println!("Comments Fetched: {}", startup.comments.len());
    println!("Max Iterations: {}", options.max_iterations);
    println!("Codex Model: {}", codex_model);
    println!("Codex Effort: {}", codex_effort);
    println!("Codex Timeout: {}s", options.codex_timeout);
    println!("Poll Interval: 30s");
    println!("Poll Timeout: 900s (per bot)");
    println!("Loop Directory: {}\n", loop_dir.display());
    print!("{}", prompt);
    Ok(())
}

fn cancel_rlcr_native(force: bool) -> Result<()> {
    let project_root = resolve_project_root()?;
    let loop_base_dir = project_root.join(".humanize/rlcr");

    let loop_dir = humanize_core::state::find_active_loop(&loop_base_dir, None)
        .ok_or_else(|| anyhow::anyhow!("No active RLCR loop found."))?;

    let state_file = loop_dir.join("state.md");
    let finalize_state_file = loop_dir.join("finalize-state.md");
    let active_state = if state_file.exists() {
        state_file
    } else if finalize_state_file.exists() {
        finalize_state_file
    } else {
        bail!("No active RLCR loop found.");
    };

    let state_content = fs::read_to_string(&active_state).unwrap_or_default();
    let state = humanize_core::state::State::from_markdown(&state_content).unwrap_or_default();

    if active_state.ends_with("finalize-state.md") && !force {
        println!("FINALIZE_NEEDS_CONFIRM");
        println!("loop_dir: {}", loop_dir.display());
        println!("current_round: {}", state.current_round);
        println!("max_iterations: {}", state.max_iterations);
        println!();
        println!("The loop is currently in Finalize Phase.");
        println!("After this phase completes, the loop will end without returning to Codex review.");
        println!();
        println!("Use --force to cancel anyway.");
        std::process::exit(2);
    }

    fs::write(loop_dir.join(".cancel-requested"), "")?;
    let pending_session = project_root.join(".humanize/.pending-session-id");
    if pending_session.exists() {
        let _ = fs::remove_file(pending_session);
    }
    humanize_core::state::State::rename_to_terminal(&active_state, "cancel")?;

    println!("CANCELLED");
    println!(
        "Cancelled RLCR loop (was at round {} of {}).",
        state.current_round, state.max_iterations
    );
    println!("State preserved as cancel-state.md");
    Ok(())
}

fn cancel_pr_native() -> Result<()> {
    let project_root = resolve_project_root()?;
    let loop_base_dir = project_root.join(".humanize/pr-loop");

    let loop_dir = newest_active_pr_loop(&loop_base_dir)
        .ok_or_else(|| anyhow::anyhow!("No active PR loop found."))?;

    let state_file = loop_dir.join("state.md");
    let state_content = fs::read_to_string(&state_file).unwrap_or_default();
    let state = humanize_core::state::State::from_markdown(&state_content).unwrap_or_default();

    fs::write(loop_dir.join(".cancel-requested"), "")?;
    humanize_core::state::State::rename_to_terminal(&state_file, "cancel")?;

    println!("CANCELLED");
    println!(
        "Cancelled PR loop for PR #{} (was at round {} of {}).",
        state.pr_number.unwrap_or_default(),
        state.current_round,
        state.max_iterations
    );
    println!("State preserved as cancel-state.md");
    Ok(())
}

fn ask_codex_native(prompt: &str, model: &str, effort: &str, timeout: u64) -> Result<()> {
    if prompt.trim().is_empty() {
        bail!("Error: No question or task provided");
    }

    let project_root = resolve_project_root()?;
    let skill_id = unique_run_id();
    let skill_dir = project_root.join(".humanize/skill").join(&skill_id);
    fs::create_dir_all(&skill_dir)?;

    let cache_dir = resolve_cache_dir(&project_root, &skill_id, &skill_dir)?;
    let stdout_file = cache_dir.join("codex-run.out");
    let stderr_file = cache_dir.join("codex-run.log");
    let cmd_file = cache_dir.join("codex-run.cmd");

    let mut options = humanize_core::codex::CodexOptions::from_env(&project_root);
    options.model = model.to_string();
    options.effort = effort.to_string();
    options.timeout_secs = timeout;

    fs::write(
        skill_dir.join("input.md"),
        format!(
            "# Ask Codex Input\n\n## Question\n\n{}\n\n## Configuration\n\n- Model: {}\n- Effort: {}\n- Timeout: {}s\n",
            prompt, model, effort, timeout
        ),
    )?;

    let args = humanize_core::codex::build_exec_args(&options);
    fs::write(
        &cmd_file,
        format!(
            "# Codex ask-codex invocation debug info\n# Working directory: {}\n# Timeout: {} seconds\n\ncodex {}\n\n# Prompt content:\n{}\n",
            project_root.display(),
            timeout,
            args.join(" "),
            prompt
        ),
    )?;

    eprintln!("ask-codex: model={} effort={} timeout={}s", model, effort, timeout);
    eprintln!("ask-codex: cache={}", cache_dir.display());
    eprintln!("ask-codex: running codex exec...");

    match humanize_core::codex::run_exec(prompt, &options) {
        Ok(result) => {
            fs::write(&stdout_file, &result.stdout)?;
            fs::write(&stderr_file, &result.stderr)?;
            fs::write(skill_dir.join("output.md"), &result.stdout)?;
            fs::write(
                skill_dir.join("metadata.md"),
                format!(
                    "---\nmodel: {}\neffort: {}\ntimeout: {}\nexit_code: 0\nstatus: success\n---\n",
                    model, effort, timeout
                ),
            )?;
            print!("{}", result.stdout);
            Ok(())
        }
        Err(humanize_core::codex::CodexError::Timeout(secs)) => {
            fs::write(
                skill_dir.join("metadata.md"),
                format!(
                    "---\nmodel: {}\neffort: {}\ntimeout: {}\nexit_code: 124\nstatus: timeout\n---\n",
                    model, effort, timeout
                ),
            )?;
            eprintln!("Error: Codex timed out after {} seconds", secs);
            std::process::exit(124);
        }
        Err(humanize_core::codex::CodexError::Exit {
            exit_code,
            stdout,
            stderr,
        }) => {
            fs::write(&stdout_file, &stdout)?;
            fs::write(&stderr_file, &stderr)?;
            fs::write(
                skill_dir.join("metadata.md"),
                format!(
                    "---\nmodel: {}\neffort: {}\ntimeout: {}\nexit_code: {}\nstatus: error\n---\n",
                    model, effort, timeout, exit_code
                ),
            )?;
            if !stderr.trim().is_empty() {
                eprintln!("{}", stderr.trim());
            }
            std::process::exit(exit_code);
        }
        Err(humanize_core::codex::CodexError::EmptyOutput) => {
            fs::write(
                skill_dir.join("metadata.md"),
                format!(
                    "---\nmodel: {}\neffort: {}\ntimeout: {}\nexit_code: 0\nstatus: empty_response\n---\n",
                    model, effort, timeout
                ),
            )?;
            bail!("Error: Codex returned empty response");
        }
        Err(err) => Err(err.into()),
    }
}

fn gen_plan_native(input: &str, output: &str) -> Result<()> {
    let input_path = PathBuf::from(input);
    let output_path = PathBuf::from(output);

    if !input_path.is_file() {
        bail!("Input file not found: {}", input_path.display());
    }

    let draft = fs::read_to_string(&input_path)?;
    if draft.trim().is_empty() {
        bail!("Input file is empty: {}", input_path.display());
    }

    let output_dir = output_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Output directory does not exist"))?;
    if !output_dir.is_dir() {
        bail!("Output directory does not exist: {}", output_dir.display());
    }
    if output_path.exists() {
        bail!("Output file already exists: {}", output_path.display());
    }

    let plugin_root = resolve_plugin_root()?;
    let template_root = humanize_core::template::template_dir(&plugin_root);
    let template = humanize_core::template::load_template(
        &template_root,
        "plan/gen-plan-template.md",
    )
    .context("Plan template file not found")?;
    let project_root = resolve_project_root()?;
    ensure_command_exists("codex", "Error: gen-plan requires codex to be installed")?;

    let mut options = humanize_core::codex::CodexOptions::from_env(&project_root);
    options.model = "gpt-5.4".to_string();
    options.effort = "xhigh".to_string();
    options.timeout_secs = 3600;

    let repo_context = build_repo_context(&project_root)?;
    let relevance = run_gen_plan_relevance_check(&draft, &repo_context, &options)?;
    if let Some(reason) = relevance.strip_prefix("NOT_RELEVANT:") {
        bail!(
            "The draft content does not appear to be related to this repository.\n{}",
            reason.trim()
        );
    }
    if !relevance.starts_with("RELEVANT:") {
        bail!("gen-plan relevance check returned invalid output: {}", relevance);
    }

    let analysis = run_gen_plan_analysis(&draft, &repo_context, &options)?;
    let clarifications = collect_gen_plan_issue_answers(&analysis)?;
    let metric_answers = collect_gen_plan_metric_answers(&analysis)?;
    let prompt = build_gen_plan_generation_prompt(
        &template,
        &draft,
        &repo_context,
        &clarifications,
        &metric_answers,
        &analysis.notes,
    );

    let result = humanize_core::codex::run_exec(&prompt, &options)
        .map_err(|err| anyhow::anyhow!("gen-plan Codex generation failed: {}", err))?;
    let mut content = strip_markdown_fence(&result.stdout).trim().to_string();
    if !content.contains("--- Original Design Draft Start ---") {
        content.push_str(&format!(
            "\n\n--- Original Design Draft Start ---\n\n{}\n\n--- Original Design Draft End ---\n",
            draft.trim_end()
        ));
    }

    if should_offer_language_unification(&analysis, &content) {
        if let Some(language) = prompt_language_unification()? {
            content = run_gen_plan_language_unification(&content, &language, &options)?;
        }
    }

    fs::write(&output_path, format!("{}\n", content.trim_end()))?;
    Ok(())
}

fn strip_markdown_fence(content: &str) -> String {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let mut lines = trimmed.lines();
    let _ = lines.next();
    let collected = lines.collect::<Vec<_>>();
    if let Some(last) = collected.last() {
        if last.trim() == "```" {
            return collected[..collected.len().saturating_sub(1)].join("\n");
        }
    }
    trimmed.to_string()
}

fn build_repo_context(project_root: &Path) -> Result<String> {
    let mut parts = Vec::new();
    parts.push(format!("Project root: {}", project_root.display()));

    let mut top_entries = fs::read_dir(project_root)?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    top_entries.sort();
    parts.push(format!("Top-level entries: {}", top_entries.join(", ")));

    for candidate in ["README.md", "CLAUDE.md", "docs/README.md"] {
        let path = project_root.join(candidate);
        if path.is_file() {
            let content = fs::read_to_string(&path).unwrap_or_default();
            let snippet = content.lines().take(80).collect::<Vec<_>>().join("\n");
            parts.push(format!("## {}\n{}", candidate, snippet));
        }
    }

    Ok(parts.join("\n\n"))
}

fn run_gen_plan_relevance_check(
    draft: &str,
    repo_context: &str,
    options: &humanize_core::codex::CodexOptions,
) -> Result<String> {
    let prompt = format!(
        "You are checking whether a draft is relevant to the current repository.\n\nRepository context:\n{}\n\nDraft:\n{}\n\nReturn exactly one line:\n- `RELEVANT: <brief explanation>`\n- `NOT_RELEVANT: <brief explanation>`\n\nBe lenient. Only return NOT_RELEVANT when the draft is clearly unrelated.",
        repo_context.trim(),
        draft.trim()
    );
    let result = humanize_core::codex::run_exec(&prompt, options)
        .map_err(|err| anyhow::anyhow!("gen-plan relevance check failed: {}", err))?;
    Ok(strip_markdown_fence(&result.stdout)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string())
}

fn run_gen_plan_analysis(
    draft: &str,
    repo_context: &str,
    options: &humanize_core::codex::CodexOptions,
) -> Result<GenPlanAnalysis> {
    let prompt = format!(
        "Analyze the draft for gen-plan.\n\nRepository context:\n{}\n\nDraft:\n{}\n\nReturn JSON only with this shape:\n{{\n  \"issues\": [{{\"question\": \"...\", \"why\": \"...\", \"options\": [\"...\", \"...\"]}}],\n  \"metrics\": [{{\"text\": \"...\", \"question\": \"...\", \"suggested_default\": \"hard|trend\"}}],\n  \"mixed_languages\": true,\n  \"language_candidates\": [\"English\", \"Chinese\"],\n  \"notes\": [\"...\"]\n}}\n\nRules:\n- `issues` should only include clarifications that materially affect the plan.\n- `metrics` should include each quantitative target or numeric threshold that needs hard-vs-trend confirmation.\n- If there are no issues or metrics, return empty arrays.\n- Be conservative: avoid spurious issues.\n- JSON only. No markdown fences.",
        repo_context.trim(),
        draft.trim()
    );
    let result = humanize_core::codex::run_exec(&prompt, options)
        .map_err(|err| anyhow::anyhow!("gen-plan analysis failed: {}", err))?;
    let json = strip_markdown_fence(&result.stdout);
    serde_json::from_str(json.trim())
        .map_err(|err| anyhow::anyhow!("gen-plan analysis returned invalid JSON: {}", err))
}

fn interactive_stdin() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn prompt_user_input(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn collect_gen_plan_issue_answers(analysis: &GenPlanAnalysis) -> Result<Vec<(String, String)>> {
    if analysis.issues.is_empty() {
        return Ok(Vec::new());
    }
    if !interactive_stdin() {
        let questions = analysis
            .issues
            .iter()
            .enumerate()
            .map(|(idx, issue)| format!("{}. {} ({})", idx + 1, issue.question, issue.why))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "gen-plan requires user clarification before continuing.\nRun this command in an interactive terminal and answer:\n{}",
            questions
        );
    }

    let mut answers = Vec::new();
    for (idx, issue) in analysis.issues.iter().enumerate() {
        eprintln!("\ngen-plan clarification {}:", idx + 1);
        eprintln!("Question: {}", issue.question);
        eprintln!("Why it matters: {}", issue.why);
        if !issue.options.is_empty() {
            eprintln!("Suggested options:");
            for option in &issue.options {
                eprintln!("- {}", option);
            }
        }
        let answer = prompt_user_input("Your answer: ")?;
        if answer.is_empty() {
            bail!("Clarification answer cannot be empty.");
        }
        answers.push((issue.question.clone(), answer));
    }
    Ok(answers)
}

fn collect_gen_plan_metric_answers(analysis: &GenPlanAnalysis) -> Result<Vec<(String, String)>> {
    if analysis.metrics.is_empty() {
        return Ok(Vec::new());
    }
    if !interactive_stdin() {
        let questions = analysis
            .metrics
            .iter()
            .enumerate()
            .map(|(idx, metric)| format!("{}. {}", idx + 1, metric.text))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "gen-plan requires metric confirmation before continuing.\nRun this command in an interactive terminal and classify each metric as `hard` or `trend`:\n{}",
            questions
        );
    }

    let mut answers = Vec::new();
    for (idx, metric) in analysis.metrics.iter().enumerate() {
        let default_hint = if metric.suggested_default.is_empty() {
            "hard/trend"
        } else {
            metric.suggested_default.as_str()
        };
        eprintln!("\ngen-plan metric confirmation {}:", idx + 1);
        eprintln!("Metric: {}", metric.text);
        if !metric.question.is_empty() {
            eprintln!("{}", metric.question);
        }
        let answer = prompt_user_input(&format!("Interpretation [{}]: ", default_hint))?;
        let normalized = if answer.is_empty() {
            metric.suggested_default.clone()
        } else {
            answer.to_ascii_lowercase()
        };
        if normalized != "hard" && normalized != "trend" {
            bail!("Metric interpretation must be `hard` or `trend`.");
        }
        answers.push((metric.text.clone(), normalized));
    }
    Ok(answers)
}

fn build_gen_plan_generation_prompt(
    template: &str,
    draft: &str,
    repo_context: &str,
    clarifications: &[(String, String)],
    metric_answers: &[(String, String)],
    notes: &[String],
) -> String {
    let clarifications_block = if clarifications.is_empty() {
        "None.".to_string()
    } else {
        clarifications
            .iter()
            .map(|(question, answer)| format!("- {} => {}", question, answer))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let metrics_block = if metric_answers.is_empty() {
        "None.".to_string()
    } else {
        metric_answers
            .iter()
            .map(|(metric, answer)| format!("- {} => {}", metric, answer))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let notes_block = if notes.is_empty() {
        "None.".to_string()
    } else {
        notes.iter().map(|note| format!("- {}", note)).collect::<Vec<_>>().join("\n")
    };

    format!(
        "Read and execute below with ultrathink.\n\nYou are generating a complete implementation plan for the current repository.\n\nRepository context:\n{}\n\nOriginal draft:\n{}\n\nClarifications from the user:\n{}\n\nMetric interpretations:\n{}\n\nAdditional analysis notes:\n{}\n\nRequirements:\n- Preserve ALL meaningful information from the draft.\n- Treat clarifications as additive, not replacements.\n- Write the final answer as raw markdown only, with no code fences.\n- Keep the final `--- Original Design Draft Start ---` and `--- Original Design Draft End ---` section at the bottom.\n- Follow the plan structure and headings from the template exactly unless a heading is clearly inapplicable.\n- Acceptance criteria must use AC-X or AC-X.Y naming.\n- Do not include time estimates.\n- Do not reference code line numbers.\n- Include positive and negative tests for each acceptance criterion.\n- Path boundaries must describe acceptable upper/lower bounds and allowed choices.\n- For each quantitative metric, reflect whether it is a hard requirement or an optimization trend.\n- Include implementation notes telling engineers not to use plan-specific markers like AC-, Milestone, Step, or Phase inside production code/comments.\n\nTemplate skeleton:\n\n{}\n",
        repo_context.trim(),
        draft.trim(),
        clarifications_block,
        metrics_block,
        notes_block,
        template.trim_end(),
    )
}

fn contains_cjk(text: &str) -> bool {
    text.chars().any(|ch| ('\u{4E00}'..='\u{9FFF}').contains(&ch))
}

fn should_offer_language_unification(analysis: &GenPlanAnalysis, content: &str) -> bool {
    analysis.mixed_languages || (contains_cjk(content) && content.chars().any(|ch| ch.is_ascii_alphabetic()))
}

fn prompt_language_unification() -> Result<Option<String>> {
    if !interactive_stdin() {
        return Ok(None);
    }
    eprintln!("\ngen-plan detected mixed-language content.");
    eprintln!("Choose language handling:");
    eprintln!("- keep");
    eprintln!("- english");
    eprintln!("- chinese");
    let answer = prompt_user_input("Language choice [keep]: ")?;
    let normalized = if answer.is_empty() {
        "keep".to_string()
    } else {
        answer.to_ascii_lowercase()
    };
    match normalized.as_str() {
        "keep" => Ok(None),
        "english" | "chinese" => Ok(Some(normalized)),
        _ => bail!("Language choice must be keep, english, or chinese."),
    }
}

fn run_gen_plan_language_unification(
    content: &str,
    language: &str,
    options: &humanize_core::codex::CodexOptions,
) -> Result<String> {
    let language_label = match language {
        "english" => "English",
        "chinese" => "Chinese",
        _ => language,
    };
    let prompt = format!(
        "Translate the following plan into {} while preserving exact meaning, structure, markdown formatting, and all technical identifiers.\n\nReturn raw markdown only.\n\n{}",
        language_label,
        content.trim()
    );
    let result = humanize_core::codex::run_exec(&prompt, options)
        .map_err(|err| anyhow::anyhow!("gen-plan language unification failed: {}", err))?;
    Ok(strip_markdown_fence(&result.stdout).trim().to_string())
}

#[derive(Debug, Default, Deserialize)]
struct StopHookInput {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    transcript_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct StopHookOutput {
    decision: String,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "systemMessage")]
    system_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GenPlanAnalysis {
    #[serde(default)]
    issues: Vec<GenPlanIssue>,
    #[serde(default)]
    metrics: Vec<GenPlanMetric>,
    #[serde(default)]
    mixed_languages: bool,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GenPlanIssue {
    question: String,
    why: String,
    #[serde(default)]
    options: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GenPlanMetric {
    text: String,
    #[serde(default)]
    question: String,
    #[serde(default)]
    suggested_default: String,
}

#[derive(Debug, Clone)]
struct PrReaction {
    user: String,
    content: String,
    created_at: String,
}

#[derive(Debug, Clone)]
struct PrTriggerComment {
    id: u64,
    created_at: String,
}

#[derive(Debug, Clone)]
struct PrReviewEvent {
    id: u64,
    source: String,
    author: String,
    created_at: String,
    body: String,
    state: Option<String>,
    path: Option<String>,
    line: Option<u64>,
}

#[derive(Debug)]
struct PrPollOutcome {
    comments: Vec<PrReviewEvent>,
    timed_out_bots: HashSet<String>,
    active_bots: Vec<String>,
}

fn handle_stop_rlcr() -> Result<()> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;
    let input: StopHookInput = serde_json::from_str(&raw).unwrap_or_default();

    let project_root = resolve_project_root()?;
    let loop_base_dir = project_root.join(".humanize/rlcr");
    let Some(loop_dir) =
        humanize_core::state::find_active_loop(&loop_base_dir, input.session_id.as_deref())
    else {
        return Ok(());
    };

    let state_file = match humanize_core::state::resolve_active_state_file(&loop_dir) {
        Some(path) => path,
        None => return Ok(()),
    };

    let is_finalize_phase = state_file.ends_with("finalize-state.md");
    let state_content = fs::read_to_string(&state_file)
        .context("Malformed state file, blocking operation for safety")?;
    let mut state = humanize_core::state::State::from_markdown_strict(&state_content)
        .map_err(|_| anyhow::anyhow!("Malformed state file, blocking operation for safety"))?;

    if let Some(reason) = validate_plan_state_schema(&state_content) {
        return emit_stop_block(&reason, Some("Loop: Blocked - state schema outdated"));
    }

    let current_branch = humanize_core::git::get_current_branch(&project_root).map_err(|_| {
        anyhow::anyhow!("Git operation failed or timed out. Cannot verify branch consistency.")
    })?;
    if !state.start_branch.trim().is_empty() && state.start_branch != current_branch {
        return emit_stop_block(
            &format!(
                "Git branch changed during RLCR loop.\n\nStarted on: {}\nCurrent: {}\n\nBranch switching is not allowed. Switch back to {} or cancel the loop.",
                state.start_branch, current_branch, state.start_branch
            ),
            Some("Loop: Blocked - branch changed"),
        );
    }

    let git_status = match git_status_porcelain_cached(&project_root) {
        Ok(status) => status,
        Err(_) => {
            let reason = render_template_or_fallback(
                "block/git-status-failed.md",
                "# Git Status Failed\n\nGit status operation failed or timed out.\n\nCannot verify repository state. Please check git status manually and try again.",
                &[("GIT_STATUS_EXIT", "unknown".to_string())],
            );
            return emit_stop_block(&reason, Some("Loop: Blocked - git status failed"));
        }
    };

    let large_files = detect_large_changed_files(&project_root, &git_status);
    if !large_files.is_empty() {
        return emit_stop_block(
            &build_large_files_reason(&large_files),
            Some("Loop: Blocked - large files detected"),
        );
    }

    let non_humanize_status = git_non_humanize_status_lines(&git_status);
    if !non_humanize_status.is_empty() {
        let reason = build_git_not_clean_reason(
            "uncommitted changes in the repository",
            &format!(
                "\nChanges detected:\n```\n{}\n```\n",
                non_humanize_status.join("\n")
            ),
        );
        return emit_stop_block(&reason, Some("Loop: Blocked - git not clean"));
    }

    if !is_finalize_phase && state.current_round >= state.max_iterations {
        humanize_core::state::State::rename_to_terminal(&state_file, "maxiter")?;
        return Ok(());
    }

    if let Some(reason) = stop_hook_plan_integrity_check(&project_root, &loop_dir, &state) {
        return emit_stop_block(&reason, Some("Loop: Blocked - plan integrity"));
    }

    let summary_file = if is_finalize_phase {
        loop_dir.join("finalize-summary.md")
    } else {
        loop_dir.join(format!("round-{}-summary.md", state.current_round))
    };
    if !summary_file.is_file() {
        return emit_stop_block(
            &format!("Summary file missing: {}", summary_file.display()),
            Some("Loop: Blocked - summary missing"),
        );
    }

    if is_finalize_phase {
        if let Some(reason) = transcript_todo_block_reason(input.transcript_path.as_deref())? {
            return emit_stop_block(&reason, Some("Loop: Blocked - incomplete tasks detected"));
        }
        humanize_core::state::State::rename_to_terminal(&state_file, "complete")?;
        return Ok(());
    }

    if let Some(reason) = goal_tracker_placeholder_reason(&loop_dir) {
        return emit_stop_block(&reason, Some("Loop: Blocked - goal tracker placeholders remain"));
    }

    if let Some(reason) = transcript_todo_block_reason(input.transcript_path.as_deref())? {
        return emit_stop_block(&reason, Some("Loop: Blocked - incomplete tasks detected"));
    }

    if state.review_started {
        return run_review_phase(&project_root, &loop_dir, &state_file, &mut state);
    }

    run_impl_phase(&project_root, &loop_dir, &state_file, &mut state, &summary_file)
}

fn run_impl_phase(
    project_root: &Path,
    loop_dir: &Path,
    state_file: &Path,
    state: &mut humanize_core::state::State,
    summary_file: &Path,
) -> Result<()> {
    let summary_content = fs::read_to_string(summary_file)?;
    let prompt = build_impl_review_prompt(loop_dir, state, &summary_content);
    let prompt_file = loop_dir.join(format!("round-{}-review-prompt.md", state.current_round));
    fs::write(&prompt_file, &prompt)?;

    let cache_dir = rlcr_cache_dir(project_root, loop_dir)?;
    let mut options = humanize_core::codex::CodexOptions::from_env(project_root);
    options.model = state.codex_model.clone();
    options.effort = state.codex_effort.clone();
    options.timeout_secs = state.codex_timeout;

    let result = match humanize_core::codex::run_exec(&prompt, &options) {
        Ok(result) => result,
        Err(err) => {
            return emit_stop_block(
                &format!("Codex review failed.\n\n{}\n\nPlease retry the exit.", err),
                Some("Loop: Blocked - codex exec failed"),
            )
        }
    };

    let review_result_file = loop_dir.join(format!("round-{}-review-result.md", state.current_round));
    fs::write(&review_result_file, &result.stdout)?;
    fs::write(
        cache_dir.join(format!("round-{}-codex-exec.log", state.current_round)),
        &result.stdout,
    )?;

    let last_line = result
        .stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .unwrap_or_default();

    if last_line == "STOP" {
        humanize_core::state::State::rename_to_terminal(state_file, "stop")?;
        return Ok(());
    }

    if last_line == "COMPLETE" {
        state.review_started = true;
        state.save(state_file)?;
        return run_review_phase(project_root, loop_dir, state_file, state);
    }

    let review_feedback = result.stdout;
    let next_round_prompt = build_next_round_prompt(loop_dir, state, &review_feedback);
    state.increment_round();
    state.save(state_file)?;
    let next_prompt = loop_dir.join(format!("round-{}-prompt.md", state.current_round));
    fs::write(&next_prompt, &next_round_prompt)?;
    emit_stop_block(&next_round_prompt, Some("Loop: Blocked - Codex feedback"))
}

fn run_review_phase(
    project_root: &Path,
    loop_dir: &Path,
    state_file: &Path,
    state: &mut humanize_core::state::State,
) -> Result<()> {
    let cache_dir = rlcr_cache_dir(project_root, loop_dir)?;
    let review_round = state.current_round + 1;
    let review_prompt_file = loop_dir.join(format!("round-{}-review-prompt.md", review_round));
    fs::write(
        &review_prompt_file,
        build_review_phase_audit_prompt(review_round, &state.base_branch),
    )?;

    let mut options = humanize_core::codex::CodexOptions::from_env(project_root);
    options.model = state.codex_model.clone();
    options.effort = "high".to_string();
    options.timeout_secs = state.codex_timeout;

    let log_file = cache_dir.join(format!("round-{}-codex-review.log", review_round));
    let combined_output = match humanize_core::codex::run_review(
        if state.base_commit.is_empty() {
            &state.base_branch
        } else {
            &state.base_commit
        },
        &options,
    ) {
        Ok(result) => {
            let combined = combine_review_output(&result.stdout, &result.stderr);
            fs::write(&log_file, &combined)?;
            combined
        }
        Err(humanize_core::codex::CodexError::Exit {
            exit_code: _,
            stdout,
            stderr,
        }) => {
            let combined = combine_review_output(&stdout, &stderr);
            fs::write(&log_file, &combined)?;
            state.review_started = true;
            state.save(state_file)?;
            return emit_stop_block(
                "The `codex review` command failed to produce valid output. Please retry the exit.",
                Some("Loop: Blocked - codex review failed"),
            );
        }
        Err(humanize_core::codex::CodexError::EmptyOutput) => {
            fs::write(&log_file, "")?;
            state.review_started = true;
            state.save(state_file)?;
            return emit_stop_block(
                "The `codex review` command produced empty output. Please retry the exit.",
                Some("Loop: Blocked - codex review empty"),
            );
        }
        Err(err) => {
            state.review_started = true;
            state.save(state_file)?;
            return emit_stop_block(
                &format!("The `codex review` command failed: {}. Please retry the exit.", err),
                Some("Loop: Blocked - codex review failed"),
            );
        }
    };

    if humanize_core::codex::contains_severity_markers(&combined_output) {
        let result_file = loop_dir.join(format!("round-{}-review-result.md", review_round));
        fs::write(&result_file, &combined_output)?;
        state.current_round = review_round;
        state.save(state_file)?;
        let next_prompt_file = loop_dir.join(format!("round-{}-prompt.md", review_round));
        let next_summary_file = loop_dir.join(format!("round-{}-summary.md", review_round));
        let review_fix_prompt = build_review_phase_fix_prompt(&combined_output, &next_summary_file);
        fs::write(&next_prompt_file, &review_fix_prompt)?;
        return emit_stop_block(&review_fix_prompt, Some("Loop: Blocked - review issues found"));
    }

    fs::rename(state_file, loop_dir.join("finalize-state.md"))?;
    let finalize_summary_file = loop_dir.join("finalize-summary.md");
    if !finalize_summary_file.exists() {
        fs::write(
            &finalize_summary_file,
            "# Finalize Summary\n\nDocument the final simplification pass here.\n",
        )?;
    }
    let finalize_prompt = build_finalize_phase_prompt(state, loop_dir, &finalize_summary_file);
    emit_stop_block(
        &finalize_prompt,
        Some("Loop: Blocked - finalize phase"),
    )
}

fn combine_review_output(stdout: &str, stderr: &str) -> String {
    match (stdout.trim(), stderr.trim()) {
        ("", "") => String::new(),
        ("", s) => format!("{}\n", s),
        (s, "") => format!("{}\n", s),
        (s1, s2) => format!("{}\n{}\n", s1, s2),
    }
}

fn stop_hook_plan_integrity_check(
    project_root: &Path,
    loop_dir: &Path,
    state: &humanize_core::state::State,
) -> Option<String> {
    let backup_plan = loop_dir.join("plan.md");
    if !backup_plan.exists() {
        return Some("Plan file backup not found in loop directory.".to_string());
    }

    let full_plan = project_root.join(&state.plan_file);
    if !full_plan.exists() {
        return Some(format!("Project plan file has been deleted.\n\nOriginal: {}", state.plan_file));
    }

    if state.plan_tracked {
        let plan_status = git_path_status_porcelain(project_root, &state.plan_file).ok()?;
        if !plan_status.trim().is_empty() {
            return Some(format!(
                "Plan file has uncommitted modifications.\n\nFile: {}\nStatus: {}",
                state.plan_file,
                plan_status.trim()
            ));
        }
    }

    let backup_content = fs::read_to_string(&backup_plan).ok()?;
    let current_content = fs::read_to_string(&full_plan).ok()?;
    if backup_content != current_content {
        return Some(format!(
            "The plan file `{}` has been modified since the RLCR loop started.",
            state.plan_file
        ));
    }

    None
}

fn goal_tracker_placeholder_reason(loop_dir: &Path) -> Option<String> {
    let goal_tracker = loop_dir.join("goal-tracker.md");
    let content = fs::read_to_string(goal_tracker).ok()?;
    let mut issues = Vec::new();
    if content.contains("[To be extracted from plan by Claude in Round 0]") {
        issues.push("**Ultimate Goal**: Still contains placeholder text");
    }
    if content.contains("[To be defined by Claude in Round 0 based on the plan]") {
        issues.push("**Acceptance Criteria**: Still contains placeholder text");
    }
    if content.contains("[To be populated by Claude based on plan]") {
        issues.push("**Active Tasks**: Still contains placeholder text");
    }
    if issues.is_empty() {
        None
    } else {
        Some(issues.join("\n"))
    }
}

fn transcript_todo_block_reason(transcript_path: Option<&str>) -> Result<Option<String>> {
    let Some(path) = transcript_path else {
        return Ok(None);
    };
    let content = fs::read_to_string(path)?;
    let mut latest_todos: Option<Vec<String>> = None;

    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        collect_incomplete_todos(&value, &mut latest_todos);
    }

    Ok(latest_todos.map(|todos| {
        format!(
            "Complete these tasks before exiting:\n\n{}",
            todos.join("\n")
        )
    }))
}

fn collect_incomplete_todos(value: &serde_json::Value, latest: &mut Option<Vec<String>>) {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("name").and_then(|v| v.as_str()) == Some("TodoWrite") {
                if let Some(todos) = map
                    .get("input")
                    .and_then(|v| v.get("todos"))
                    .and_then(|v| v.as_array())
                {
                    let incomplete = todos
                        .iter()
                        .filter_map(|todo| {
                            let status = todo.get("status")?.as_str()?;
                            if status == "completed" {
                                None
                            } else {
                                Some(
                                    todo.get("content")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("Incomplete task")
                                        .to_string(),
                                )
                            }
                        })
                        .collect::<Vec<_>>();
                    if !incomplete.is_empty() {
                        *latest = Some(incomplete);
                    } else {
                        *latest = None;
                    }
                }
            }
            for child in map.values() {
                collect_incomplete_todos(child, latest);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_incomplete_todos(child, latest);
            }
        }
        _ => {}
    }
}

fn emit_stop_block(reason: &str, system_message: Option<&str>) -> Result<()> {
    let output = StopHookOutput {
        decision: "block".to_string(),
        reason: reason.to_string(),
        system_message: system_message.map(|msg| msg.to_string()),
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn rlcr_cache_dir(project_root: &Path, loop_dir: &Path) -> Result<PathBuf> {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{}/.cache", h)))
        .unwrap_or_else(|| ".cache".to_string());
    let loop_timestamp = loop_dir
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown-loop");
    let dir = PathBuf::from(base)
        .join("humanize")
        .join(sanitize_path_component(project_root))
        .join(loop_timestamp);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn resolve_project_root() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("CLAUDE_PROJECT_DIR") {
        return Ok(PathBuf::from(dir));
    }

    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        if output.status.success() {
            let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !root.is_empty() {
                return Ok(PathBuf::from(root));
            }
        }
    }

    Ok(std::env::current_dir()?)
}

fn resolve_plugin_root() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("CLAUDE_PLUGIN_ROOT") {
        return Ok(PathBuf::from(dir));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            if let Some(root) = bin_dir.parent() {
                if root.join("prompt-template").is_dir() {
                    return Ok(root.to_path_buf());
                }
            }
        }
    }

    let cwd = std::env::current_dir()?;
    if cwd.join("prompt-template").is_dir() {
        return Ok(cwd);
    }

    bail!("Could not determine CLAUDE_PLUGIN_ROOT");
}

fn newest_active_pr_loop(base_dir: &Path) -> Option<PathBuf> {
    if !base_dir.is_dir() {
        return None;
    }

    let mut dirs = fs::read_dir(base_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.reverse();

    dirs.into_iter().find(|dir| dir.join("state.md").exists())
}

#[derive(Debug, Clone)]
struct PrCommitInfo {
    latest_commit_sha: String,
    latest_commit_at: String,
}

#[derive(Debug, Clone)]
struct StartupCaseInfo {
    case_num: u32,
    comments: Vec<PrComment>,
}

#[derive(Debug, Clone)]
struct PrComment {
    author: String,
    created_at: String,
    body: String,
    source: &'static str,
}

fn ensure_command_exists(cmd: &str, message: &str) -> Result<()> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let exists = std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(cmd);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            let candidate_exe = dir.join(format!("{}.exe", cmd));
            candidate_exe.is_file()
        }
        #[cfg(not(windows))]
        {
            false
        }
    });
    if !exists {
        bail!("{}", message);
    }
    Ok(())
}

fn ensure_gh_auth(project_root: &Path) -> Result<()> {
    let status = Command::new("gh")
        .args(["auth", "status"])
        .current_dir(project_root)
        .status()?;
    if !status.success() {
        bail!("Error: GitHub CLI is not authenticated");
    }
    Ok(())
}

fn gh_current_repo(project_root: &Path) -> Result<String> {
    let output = Command::new("gh")
        .args(["repo", "view", "--json", "owner,name", "-q", ".owner.login + \"/\" + .name"])
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        bail!("Error: Failed to get current repository");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn gh_current_user(project_root: &Path) -> Result<String> {
    let output = Command::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        bail!("Error: Failed to get current GitHub user");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn gh_detect_pr_number(project_root: &Path) -> Result<u32> {
    let output = Command::new("gh")
        .args(["pr", "view", "--json", "number,url", "-q", ".number,.url"])
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        bail!("Error: No pull request found for the current branch");
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut lines = stdout.lines();
    let number = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("Error: Missing PR number"))?
        .trim()
        .parse::<u32>()
        .context("Error: Invalid PR number from gh CLI")?;
    Ok(number)
}

fn gh_pr_state(project_root: &Path, pr_number: u32) -> Result<String> {
    let output = Command::new("gh")
        .args(["pr", "view", &pr_number.to_string(), "--json", "state", "-q", ".state"])
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        bail!("Error: Failed to fetch PR state");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn gh_pr_commit_info(project_root: &Path, pr_number: u32) -> Result<PrCommitInfo> {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "headRefOid,commits",
            "--jq",
            "{sha: .headRefOid, date: (.commits | sort_by(.committedDate) | last | .committedDate)}",
        ])
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        bail!("Error: Failed to fetch PR commit info");
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(PrCommitInfo {
        latest_commit_sha: value
            .get("sha")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        latest_commit_at: value
            .get("date")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

fn build_active_bots(options: &SetupPrOptions) -> Vec<String> {
    let mut bots = Vec::new();
    if options.claude {
        bots.push("claude".to_string());
    }
    if options.codex {
        bots.push("codex".to_string());
    }
    bots
}

fn build_bot_mention_string(bots: &[String]) -> String {
    bots.iter()
        .map(|bot| format!("@{}", bot))
        .collect::<Vec<_>>()
        .join(" ")
}

fn bot_author(bot: &str) -> &str {
    match bot {
        "codex" => "chatgpt-codex-connector[bot]",
        "claude" => "claude[bot]",
        _ => bot,
    }
}

fn gh_fetch_comments(project_root: &Path, repo: &str, pr_number: u32) -> Result<Vec<PrComment>> {
    let mut comments = Vec::new();
    comments.extend(gh_fetch_comment_source(
        project_root,
        &format!("repos/{}/issues/{}/comments", repo, pr_number),
        "issue_comment",
    )?);
    comments.extend(gh_fetch_comment_source(
        project_root,
        &format!("repos/{}/pulls/{}/comments", repo, pr_number),
        "review_comment",
    )?);
    comments.extend(gh_fetch_review_source(
        project_root,
        &format!("repos/{}/pulls/{}/reviews", repo, pr_number),
    )?);
    comments.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(comments)
}

fn gh_fetch_comment_source(project_root: &Path, endpoint: &str, source: &'static str) -> Result<Vec<PrComment>> {
    let output = Command::new("gh")
        .args(["api", endpoint])
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let values: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap_or_default();
    Ok(values
        .into_iter()
        .map(|value| PrComment {
            author: value
                .get("user")
                .and_then(|v| v.get("login"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            created_at: value
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            body: value
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            source,
        })
        .collect())
}

fn gh_fetch_review_source(project_root: &Path, endpoint: &str) -> Result<Vec<PrComment>> {
    let output = Command::new("gh")
        .args(["api", endpoint])
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let values: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap_or_default();
    Ok(values
        .into_iter()
        .map(|value| {
            let state = value
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let body = value
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            PrComment {
                author: value
                    .get("user")
                    .and_then(|v| v.get("login"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                created_at: value
                    .get("submitted_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                body: if body.is_empty() {
                    state.to_string()
                } else {
                    format!("{} ({})", body, state)
                },
                source: "pr_review",
            }
        })
        .collect())
}

fn gh_startup_case(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
    bots: &[String],
    latest_commit_at: &str,
) -> Result<StartupCaseInfo> {
    let comments = gh_fetch_comments(project_root, repo, pr_number)?;
    let mut commented = Vec::new();
    let mut missing = Vec::new();
    let mut stale = Vec::new();

    for bot in bots {
        let author = bot_author(bot);
        let bot_comments = comments
            .iter()
            .filter(|comment| comment.author == author)
            .collect::<Vec<_>>();
        if bot_comments.is_empty() {
            missing.push(bot.clone());
            continue;
        }
        commented.push(bot.clone());
        if let Some(newest) = bot_comments.iter().map(|c| c.created_at.as_str()).max() {
            if !latest_commit_at.is_empty() && newest < latest_commit_at {
                stale.push(bot.clone());
            }
        }
    }

    let case_num = if commented.is_empty() {
        1
    } else if !missing.is_empty() && stale.is_empty() {
        2
    } else if missing.is_empty() && stale.is_empty() {
        3
    } else if missing.is_empty() {
        4
    } else {
        5
    };

    Ok(StartupCaseInfo { case_num, comments })
}

fn format_initial_pr_comments(comments: &[PrComment]) -> String {
    if comments.is_empty() {
        return "# PR Comments\n\n*No comments found.*\n".to_string();
    }
    let mut out = String::from("# PR Comments\n\n");
    for comment in comments {
        out.push_str(&format!(
            "## {} ({})\n\n- Author: {}\n- Time: {}\n\n{}\n\n",
            comment.source, comment.author, comment.author, comment.created_at, comment.body
        ));
    }
    out
}

fn gh_find_latest_user_comment(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
    user: &str,
) -> Result<Option<(u64, String)>> {
    let output = Command::new("gh")
        .args(["api", &format!("repos/{}/issues/{}/comments", repo, pr_number)])
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let values: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap_or_default();
    let latest = values
        .into_iter()
        .filter(|value| {
            value.get("user")
                .and_then(|v| v.get("login"))
                .and_then(|v| v.as_str())
                == Some(user)
        })
        .max_by(|a, b| {
            a.get("created_at")
                .and_then(|v| v.as_str())
                .cmp(&b.get("created_at").and_then(|v| v.as_str()))
        });
    Ok(latest.and_then(|value| {
        Some((
            value.get("id")?.as_u64()?,
            value.get("created_at")?.as_str()?.to_string(),
        ))
    }))
}

fn build_pr_goal_tracker(
    pr_number: u32,
    branch: &str,
    bots: &[String],
    startup_case: u32,
    started_at: &str,
) -> String {
    let active_bots = bots.join(", ");
    format!(
        "# PR Review Goal Tracker\n\n## PR Information\n\n- **PR Number:** #{}\n- **Branch:** {}\n- **Started:** {}\n- **Monitored Bots:** {}\n- **Startup Case:** {}\n\n## Ultimate Goal\n\nGet all monitored bot reviewers ({}) to approve this PR.\n\n## Issue Summary\n\n| Round | Reviewer | Issues Found | Issues Resolved | Status |\n|-------|----------|--------------|-----------------|--------|\n| 0     | -        | 0            | 0               | Initial |\n\n## Total Statistics\n\n- Total Issues Found: 0\n- Total Issues Resolved: 0\n- Remaining: 0\n\n## Issue Log\n\n### Round 0\n*Awaiting initial reviews*\n\nStarted: {}\nStartup Case: {}\n",
        pr_number,
        branch,
        started_at,
        active_bots,
        startup_case,
        active_bots,
        started_at,
        startup_case
    )
}

fn build_pr_round_0_prompt(
    pr_number: u32,
    branch: &str,
    bots: &[String],
    comment_file: &Path,
    resolve_path: &Path,
    no_comments: bool,
) -> String {
    let mention = build_bot_mention_string(bots);
    let comments = fs::read_to_string(comment_file).unwrap_or_default();
    let task = if no_comments {
        format!(
            "\n## Your Task\n\nThis PR has no review comments yet. Wait for the first bot review, then write your summary to @{} and try to exit.\n",
            resolve_path.display()
        )
    } else {
        format!(
            "\n## Your Task\n\nAddress the comments above, push your changes, and write your resolution summary to @{}.\nUse this mention string for re-review: `{}`.\n",
            resolve_path.display(),
            mention
        )
    };
    format!(
        "Read and execute below with ultrathink\n\n## PR Review Loop (Round 0)\n\n- PR Number: #{}\n- Branch: {}\n- Active Bots: {}\n\n{}\n{}\n",
        pr_number,
        branch,
        bots.join(", "),
        comments,
        task
    )
}

fn template_vars(pairs: &[(&str, String)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn render_safe_template(
    template_root: &Path,
    template_name: &str,
    fallback: &str,
    vars: &[(&str, String)],
) -> String {
    humanize_core::template::load_and_render_safe(
        template_root,
        template_name,
        fallback,
        &template_vars(vars),
    )
}

fn resolve_template_root() -> Option<PathBuf> {
    resolve_plugin_root()
        .ok()
        .map(|plugin_root| humanize_core::template::template_dir(&plugin_root))
}

fn render_template_or_fallback(
    template_name: &str,
    fallback: &str,
    vars: &[(&str, String)],
) -> String {
    if let Some(template_root) = resolve_template_root() {
        render_safe_template(&template_root, template_name, fallback, vars)
    } else {
        humanize_core::template::render_template(fallback, &template_vars(vars))
    }
}

const MAX_LOOP_FILE_LINES: usize = 2000;

fn git_status_porcelain_cached(project_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["-C", project_root.to_str().unwrap_or(".")])
        .args(["status", "--porcelain"])
        .output()?;
    if !output.status.success() {
        bail!("git status --porcelain failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_non_humanize_status_lines(status: &str) -> Vec<String> {
    status
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.contains(".humanize"))
        .map(ToString::to_string)
        .collect()
}

fn git_changed_paths(status: &str) -> Vec<String> {
    status
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_end();
            if trimmed.len() < 4 {
                return None;
            }
            let mut path = trimmed[3..].to_string();
            if let Some((_, new_path)) = path.rsplit_once(" -> ") {
                path = new_path.to_string();
            }
            Some(path)
        })
        .collect()
}

fn detect_large_changed_files(project_root: &Path, status: &str) -> Vec<(String, usize, &'static str)> {
    let mut seen = HashSet::new();
    let mut large_files = Vec::new();
    for relative in git_changed_paths(status) {
        if !seen.insert(relative.clone()) {
            continue;
        }
        let file_path = project_root.join(&relative);
        if !file_path.is_file() {
            continue;
        }

        let extension = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let file_type = match extension.as_str() {
            "py" | "js" | "ts" | "tsx" | "jsx" | "java" | "c" | "cpp" | "cc" | "cxx" | "h"
            | "hpp" | "cs" | "go" | "rs" | "rb" | "php" | "swift" | "kt" | "kts" | "scala"
            | "sh" | "bash" | "zsh" => "code",
            "md" | "rst" | "txt" | "adoc" | "asciidoc" => "documentation",
            _ => continue,
        };

        let line_count = fs::read_to_string(&file_path)
            .map(|content| content.lines().count())
            .unwrap_or(0);
        if line_count > MAX_LOOP_FILE_LINES {
            large_files.push((relative, line_count, file_type));
        }
    }
    large_files
}

fn build_large_files_reason(large_files: &[(String, usize, &'static str)]) -> String {
    let rendered_files = large_files
        .iter()
        .map(|(path, line_count, file_type)| {
            format!("- `{}`: {} lines ({} file)", path, line_count, file_type)
        })
        .collect::<Vec<_>>()
        .join("\n");
    render_template_or_fallback(
        "block/large-files.md",
        "# Large Files Detected\n\nFiles exceeding {{MAX_LINES}} lines:\n{{LARGE_FILES}}\n\nSplit these into smaller modules before continuing.",
        &[
            ("MAX_LINES", MAX_LOOP_FILE_LINES.to_string()),
            ("LARGE_FILES", rendered_files),
        ],
    )
}

fn build_git_not_clean_reason(git_issues: &str, special_notes: &str) -> String {
    render_template_or_fallback(
        "block/git-not-clean.md",
        "# Git Not Clean\n\nYou are trying to stop, but you have {{GIT_ISSUES}}.\n{{SPECIAL_NOTES}}\nCommit your changes and try again.",
        &[
            ("GIT_ISSUES", git_issues.to_string()),
            ("SPECIAL_NOTES", special_notes.to_string()),
        ],
    )
}

fn rlcr_goal_tracker_update_section(loop_dir: &Path) -> String {
    render_template_or_fallback(
        "codex/goal-tracker-update-section.md",
        "## Goal Tracker Updates\nIf Claude's summary includes a Goal Tracker Update Request section, apply the requested changes to {{GOAL_TRACKER_FILE}}.",
        &[(
            "GOAL_TRACKER_FILE",
            loop_dir.join("goal-tracker.md").display().to_string(),
        )],
    )
}

fn is_full_alignment_round(current_round: u32, full_review_round: u32) -> bool {
    let normalized = if full_review_round < 2 { 5 } else { full_review_round };
    current_round % normalized == normalized - 1
}

fn detect_open_question(review_content: &str) -> bool {
    review_content.lines().any(|line| {
        line.len() < 40 && line.contains("Open Question")
    })
}

fn build_impl_review_prompt(
    loop_dir: &Path,
    state: &humanize_core::state::State,
    summary_content: &str,
) -> String {
    let full_alignment = is_full_alignment_round(state.current_round, state.full_review_round);
    let loop_timestamp = loop_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown-loop");
    let current_round = state.current_round;
    let review_result_file = loop_dir
        .join(format!("round-{}-review-result.md", current_round))
        .display()
        .to_string();
    let vars = vec![
        ("CURRENT_ROUND", current_round.to_string()),
        ("PLAN_FILE", state.plan_file.clone()),
        (
            "PROMPT_FILE",
            loop_dir
                .join(format!("round-{}-prompt.md", current_round))
                .display()
                .to_string(),
        ),
        ("SUMMARY_CONTENT", summary_content.to_string()),
        (
            "GOAL_TRACKER_FILE",
            loop_dir.join("goal-tracker.md").display().to_string(),
        ),
        ("DOCS_PATH", "docs".to_string()),
        (
            "GOAL_TRACKER_UPDATE_SECTION",
            rlcr_goal_tracker_update_section(loop_dir),
        ),
        ("COMPLETED_ITERATIONS", (current_round + 1).to_string()),
        ("LOOP_TIMESTAMP", loop_timestamp.to_string()),
        (
            "PREV_ROUND",
            if current_round > 0 {
                (current_round - 1).to_string()
            } else {
                "0".to_string()
            },
        ),
        (
            "PREV_PREV_ROUND",
            if current_round > 1 {
                (current_round - 2).to_string()
            } else {
                "0".to_string()
            },
        ),
        ("REVIEW_RESULT_FILE", review_result_file),
    ];

    if full_alignment {
        render_template_or_fallback(
            "codex/full-alignment-review.md",
            "# Full Alignment Review (Round {{CURRENT_ROUND}})\n\nReview Claude's work against the plan and goal tracker. Check all goals are being met.\n\n## Claude's Summary\n{{SUMMARY_CONTENT}}\n\n{{GOAL_TRACKER_UPDATE_SECTION}}\n\nWrite your review to {{REVIEW_RESULT_FILE}}. End with COMPLETE if done, or list issues.",
            &vars,
        )
    } else {
        render_template_or_fallback(
            "codex/regular-review.md",
            "# Code Review (Round {{CURRENT_ROUND}})\n\nReview Claude's work for this round.\n\n## Claude's Summary\n{{SUMMARY_CONTENT}}\n\n{{GOAL_TRACKER_UPDATE_SECTION}}\n\nWrite your review to {{REVIEW_RESULT_FILE}}. End with COMPLETE if done, or list issues.",
            &vars,
        )
    }
}

fn build_next_round_prompt(
    loop_dir: &Path,
    state: &humanize_core::state::State,
    review_content: &str,
) -> String {
    let next_round = state.current_round + 1;
    let next_summary_file = loop_dir.join(format!("round-{}-summary.md", next_round));
    let full_alignment = is_full_alignment_round(state.current_round, state.full_review_round);

    let mut prompt = render_template_or_fallback(
        "claude/next-round-prompt.md",
        "Your work is not finished. Read and execute the below with ultrathink.\n\n## Original Implementation Plan\n\n@{{PLAN_FILE}}\n\nBelow is Codex's review result:\n{{REVIEW_CONTENT}}\n\n## Goal Tracker Reference\n@{{GOAL_TRACKER_FILE}}\n",
        &[
            ("PLAN_FILE", state.plan_file.clone()),
            ("REVIEW_CONTENT", review_content.to_string()),
            (
                "GOAL_TRACKER_FILE",
                loop_dir.join("goal-tracker.md").display().to_string(),
            ),
        ],
    );

    if state.ask_codex_question && detect_open_question(review_content) {
        let notice = render_template_or_fallback(
            "claude/open-question-notice.md",
            "**IMPORTANT**: Codex has found Open Question(s). You must use `AskUserQuestion` to clarify those questions with user first, before proceeding to resolve any other Codex findings.",
            &[],
        );
        prompt.push_str("\n\n");
        prompt.push_str(&notice);
    }

    if full_alignment {
        let post_alignment = render_template_or_fallback(
            "claude/post-alignment-action-items.md",
            "### Post-Alignment Check Action Items\n\nPay special attention to forgotten items, AC status, and unjustified deferrals.",
            &[],
        );
        prompt.push_str("\n\n");
        prompt.push_str(&post_alignment);
    }

    let footer = render_template_or_fallback(
        "claude/next-round-footer.md",
        "## Before Exiting\nCommit your changes and write summary to {{NEXT_SUMMARY_FILE}}",
        &[("NEXT_SUMMARY_FILE", next_summary_file.display().to_string())],
    );
    prompt.push_str("\n\n");
    prompt.push_str(&footer);

    if state.push_every_round {
        prompt.push_str("\n");
        prompt.push_str(&render_template_or_fallback(
            "claude/push-every-round-note.md",
            "Note: Since `--push-every-round` is enabled, you must push your commits to remote after each round.",
            &[],
        ));
    }

    prompt.push_str("\n");
    prompt.push_str(&render_template_or_fallback(
        "claude/goal-tracker-update-request.md",
        "Include a Goal Tracker Update Request section in your summary if needed.",
        &[],
    ));

    prompt
}

fn build_review_phase_fix_prompt(review_content: &str, summary_file: &Path) -> String {
    render_template_or_fallback(
        "claude/review-phase-prompt.md",
        "# Code Review Findings\n\n{{REVIEW_CONTENT}}\n\nWrite your summary to: `{{SUMMARY_FILE}}`",
        &[
            ("REVIEW_CONTENT", review_content.to_string()),
            ("SUMMARY_FILE", summary_file.display().to_string()),
        ],
    )
}

fn build_review_phase_audit_prompt(
    review_round: u32,
    base_branch: &str,
) -> String {
    render_template_or_fallback(
        "codex/code-review-phase.md",
        "# Code Review Phase - Round {{REVIEW_ROUND}}\n\nBase: {{BASE_BRANCH}}",
        &[
            ("REVIEW_ROUND", review_round.to_string()),
            ("BASE_BRANCH", base_branch.to_string()),
            ("TIMESTAMP", now_utc_string()),
        ],
    )
}

fn build_finalize_phase_prompt(
    state: &humanize_core::state::State,
    loop_dir: &Path,
    finalize_summary_file: &Path,
) -> String {
    render_template_or_fallback(
        "claude/finalize-phase-prompt.md",
        "# Finalize Phase\n\nYou are now in the Finalize Phase.\n\nWrite your finalize summary to: {{FINALIZE_SUMMARY_FILE}}",
        &[
            ("BASE_BRANCH", state.base_branch.clone()),
            ("START_BRANCH", state.start_branch.clone()),
            ("PLAN_FILE", state.plan_file.clone()),
            (
                "GOAL_TRACKER_FILE",
                loop_dir.join("goal-tracker.md").display().to_string(),
            ),
            (
                "FINALIZE_SUMMARY_FILE",
                finalize_summary_file.display().to_string(),
            ),
        ],
    )
}

fn parse_json_value_stream(bytes: &[u8]) -> Result<Vec<serde_json::Value>> {
    let mut values = Vec::new();
    let stream = serde_json::Deserializer::from_slice(bytes).into_iter::<serde_json::Value>();
    for value in stream {
        let value = value?;
        match value {
            serde_json::Value::Array(items) => values.extend(items),
            other => values.push(other),
        }
    }
    Ok(values)
}

fn gh_output(project_root: &Path, args: &[&str]) -> Result<std::process::Output> {
    Ok(Command::new("gh").args(args).current_dir(project_root).output()?)
}

fn gh_api_values(project_root: &Path, endpoint: &str) -> Result<Vec<serde_json::Value>> {
    let output = gh_output(project_root, &["api", endpoint, "--paginate"])?;
    if !output.status.success() {
        bail!(
            "GitHub API failed for {}: {}",
            endpoint,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_json_value_stream(&output.stdout)
}

fn gh_current_repo_json(project_root: &Path) -> Result<String> {
    let output = gh_output(project_root, &["repo", "view", "--json", "owner,name"])?;
    if !output.status.success() {
        bail!("Error: Failed to get current repository");
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let owner = value
        .get("owner")
        .and_then(|v| v.get("login"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let name = value.get("name").and_then(|v| v.as_str()).unwrap_or_default();
    if owner.is_empty() || name.is_empty() {
        bail!("Error: Failed to parse current repository");
    }
    Ok(format!("{}/{}", owner, name))
}

fn gh_parent_repo(project_root: &Path) -> Result<Option<String>> {
    let output = gh_output(project_root, &["repo", "view", "--json", "parent"])?;
    if !output.status.success() {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let parent = value.get("parent").unwrap_or(&serde_json::Value::Null);
    let owner = parent
        .get("owner")
        .and_then(|v| v.get("login"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let name = parent.get("name").and_then(|v| v.as_str()).unwrap_or_default();
    if owner.is_empty() || name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!("{}/{}", owner, name)))
    }
}

fn gh_repo_contains_pr(project_root: &Path, repo: &str, pr_number: u32) -> bool {
    gh_output(
        project_root,
        &["pr", "view", &pr_number.to_string(), "--repo", repo, "--json", "number"],
    )
    .map(|output| output.status.success())
    .unwrap_or(false)
}

fn gh_resolve_pr_repo(project_root: &Path, pr_number: u32) -> Result<String> {
    let current_repo = gh_current_repo_json(project_root)?;
    if gh_repo_contains_pr(project_root, &current_repo, pr_number) {
        return Ok(current_repo);
    }

    if let Some(parent_repo) = gh_parent_repo(project_root)? {
        if gh_repo_contains_pr(project_root, &parent_repo, pr_number) {
            return Ok(parent_repo);
        }
    }

    Ok(current_repo)
}

fn gh_pr_state_in_repo(project_root: &Path, repo: &str, pr_number: u32) -> Result<String> {
    let output = gh_output(
        project_root,
        &[
            "pr",
            "view",
            &pr_number.to_string(),
            "--repo",
            repo,
            "--json",
            "state",
        ],
    )?;
    if !output.status.success() {
        bail!("Error: Failed to fetch PR state");
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(value
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string())
}

fn gh_pr_commit_info_in_repo(project_root: &Path, repo: &str, pr_number: u32) -> Result<PrCommitInfo> {
    let output = gh_output(
        project_root,
        &[
            "pr",
            "view",
            &pr_number.to_string(),
            "--repo",
            repo,
            "--json",
            "headRefOid,commits",
        ],
    )?;
    if !output.status.success() {
        bail!("Error: Failed to fetch PR commit info");
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let latest_commit_at = value
        .get("commits")
        .and_then(|v| v.as_array())
        .and_then(|items| {
            items.iter()
                .filter_map(|item| item.get("committedDate").and_then(|v| v.as_str()))
                .max()
        })
        .unwrap_or_default()
        .to_string();
    Ok(PrCommitInfo {
        latest_commit_sha: value
            .get("headRefOid")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        latest_commit_at,
    })
}

fn map_author_to_bot(author: &str) -> Option<String> {
    match author {
        "chatgpt-codex-connector[bot]" => Some("codex".to_string()),
        "claude[bot]" => Some("claude".to_string()),
        other if other.ends_with("[bot]") => Some(other.trim_end_matches("[bot]").to_string()),
        _ => None,
    }
}

fn parse_iso_timestamp_epoch(timestamp: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.timestamp())
}

fn now_utc_string() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn pr_requires_trigger(current_round: u32, startup_case: &str, new_commits_detected: bool) -> bool {
    if current_round > 0 {
        true
    } else if new_commits_detected {
        true
    } else {
        matches!(startup_case, "4" | "5")
    }
}

fn sanitize_bot_list(bots: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    bots.iter()
        .filter_map(|bot| {
            if seen.insert(bot.clone()) {
                Some(bot.clone())
            } else {
                None
            }
        })
        .collect()
}

fn gh_detect_trigger_comment(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
    current_user: &str,
    configured_bots: &[String],
    after_timestamp: Option<&str>,
) -> Result<Option<PrTriggerComment>> {
    let values = gh_api_values(
        project_root,
        &format!("repos/{}/issues/{}/comments", repo, pr_number),
    )?;
    let mut latest: Option<PrTriggerComment> = None;
    for value in values {
        let author = value
            .get("user")
            .and_then(|v| v.get("login"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if author != current_user {
            continue;
        }
        let body = value.get("body").and_then(|v| v.as_str()).unwrap_or_default();
        if !configured_bots
            .iter()
            .any(|bot| body.to_lowercase().contains(&format!("@{}", bot.to_lowercase())))
        {
            continue;
        }
        let created_at = value
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if let Some(after) = after_timestamp {
            if !after.is_empty() && created_at.as_str() < after {
                continue;
            }
        }
        let Some(id) = value.get("id").and_then(|v| v.as_u64()) else {
            continue;
        };
        let candidate = PrTriggerComment { id, created_at };
        if latest
            .as_ref()
            .map(|current| candidate.created_at > current.created_at)
            .unwrap_or(true)
        {
            latest = Some(candidate);
        }
    }
    Ok(latest)
}

fn gh_issue_reactions(project_root: &Path, repo: &str, issue_number: u32) -> Result<Vec<PrReaction>> {
    let values = gh_api_values(
        project_root,
        &format!("repos/{}/issues/{}/reactions", repo, issue_number),
    )?;
    Ok(values
        .into_iter()
        .filter_map(|value| {
            Some(PrReaction {
                user: value.get("user")?.get("login")?.as_str()?.to_string(),
                content: value.get("content")?.as_str()?.to_string(),
                created_at: value.get("created_at")?.as_str()?.to_string(),
            })
        })
        .collect())
}

fn gh_comment_reactions(project_root: &Path, repo: &str, comment_id: &str) -> Result<Vec<PrReaction>> {
    let values = gh_api_values(
        project_root,
        &format!("repos/{}/issues/comments/{}/reactions", repo, comment_id),
    )?;
    Ok(values
        .into_iter()
        .filter_map(|value| {
            Some(PrReaction {
                user: value.get("user")?.get("login")?.as_str()?.to_string(),
                content: value.get("content")?.as_str()?.to_string(),
                created_at: value.get("created_at")?.as_str()?.to_string(),
            })
        })
        .collect())
}

fn gh_find_codex_thumbsup(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
    after_timestamp: Option<&str>,
) -> Result<Option<PrReaction>> {
    let mut matches = gh_issue_reactions(project_root, repo, pr_number)?
        .into_iter()
        .filter(|reaction| {
            reaction.user == "chatgpt-codex-connector[bot]"
                && reaction.content == "+1"
                && after_timestamp
                    .map(|after| after.is_empty() || reaction.created_at.as_str() >= after)
                    .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(matches.pop())
}

fn gh_wait_for_claude_eyes(
    project_root: &Path,
    repo: &str,
    comment_id: &str,
    retry_count: usize,
    delay: Duration,
) -> Result<Option<PrReaction>> {
    for attempt in 0..retry_count {
        if attempt > 0 {
            sleep(delay);
        }
        if let Ok(reactions) = gh_comment_reactions(project_root, repo, comment_id) {
            if let Some(reaction) = reactions.into_iter().find(|reaction| {
                reaction.user == "claude[bot]" && reaction.content == "eyes"
            }) {
                return Ok(Some(reaction));
            }
        }
    }
    Ok(None)
}

fn gh_fetch_review_events(project_root: &Path, repo: &str, pr_number: u32) -> Result<Vec<PrReviewEvent>> {
    let issue_values = gh_api_values(
        project_root,
        &format!("repos/{}/issues/{}/comments", repo, pr_number),
    )
    .unwrap_or_default();
    let review_comment_values = gh_api_values(
        project_root,
        &format!("repos/{}/pulls/{}/comments", repo, pr_number),
    )
    .unwrap_or_default();
    let review_values = gh_api_values(
        project_root,
        &format!("repos/{}/pulls/{}/reviews", repo, pr_number),
    )
    .unwrap_or_default();

    let mut events = Vec::new();

    for value in issue_values {
        let Some(id) = value.get("id").and_then(|v| v.as_u64()) else {
            continue;
        };
        events.push(PrReviewEvent {
            id,
            source: "issue_comment".to_string(),
            author: value
                .get("user")
                .and_then(|v| v.get("login"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            created_at: value
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            body: value
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            state: None,
            path: None,
            line: None,
        });
    }

    for value in review_comment_values {
        let Some(id) = value.get("id").and_then(|v| v.as_u64()) else {
            continue;
        };
        events.push(PrReviewEvent {
            id,
            source: "review_comment".to_string(),
            author: value
                .get("user")
                .and_then(|v| v.get("login"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            created_at: value
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            body: value
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            state: None,
            path: value.get("path").and_then(|v| v.as_str()).map(ToString::to_string),
            line: value
                .get("line")
                .or_else(|| value.get("original_line"))
                .and_then(|v| v.as_u64()),
        });
    }

    for value in review_values {
        let Some(id) = value.get("id").and_then(|v| v.as_u64()) else {
            continue;
        };
        let state = value
            .get("state")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        let body = value.get("body").and_then(|v| v.as_str()).unwrap_or_default();
        let review_body = if body.is_empty() {
            format!("[Review state: {}]", state.as_deref().unwrap_or("UNKNOWN"))
        } else {
            body.to_string()
        };
        events.push(PrReviewEvent {
            id,
            source: "pr_review".to_string(),
            author: value
                .get("user")
                .and_then(|v| v.get("login"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            created_at: value
                .get("submitted_at")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            body: review_body,
            state,
            path: None,
            line: None,
        });
    }

    events.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(events)
}

fn filter_review_events(
    events: Vec<PrReviewEvent>,
    after_timestamp: &str,
    watched_bots: &[String],
) -> Vec<PrReviewEvent> {
    let authors = watched_bots
        .iter()
        .map(|bot| bot_author(bot).to_string())
        .collect::<HashSet<_>>();
    events
        .into_iter()
        .filter(|event| event.created_at.as_str() >= after_timestamp && authors.contains(&event.author))
        .collect()
}

fn format_pr_review_comments_markdown(
    next_round: u32,
    configured_bots: &[String],
    current_active_bots: &[String],
    comments: &[PrReviewEvent],
) -> String {
    let mut out = format!(
        "# Bot Reviews (Round {})\n\nFetched at: {}\nConfigured bots: {}\nCurrently active: {}\n\n---\n\n",
        next_round,
        now_utc_string(),
        configured_bots.join(", "),
        current_active_bots.join(", ")
    );

    for bot in configured_bots {
        let author = bot_author(bot);
        out.push_str(&format!("## Comments from {}\n\n", author));
        let bot_comments = comments
            .iter()
            .filter(|comment| comment.author == author)
            .collect::<Vec<_>>();
        if bot_comments.is_empty() {
            out.push_str("*No new comments from this bot.*\n\n---\n\n");
            continue;
        }

        for comment in bot_comments {
            out.push_str("### Comment\n\n");
            out.push_str(&format!("- **Type**: {}\n", comment.source.replace('_', " ")));
            out.push_str(&format!("- **Time**: {}\n", comment.created_at));
            if let Some(path) = &comment.path {
                if let Some(line) = comment.line {
                    out.push_str(&format!("- **File**: `{}` (line {})\n", path, line));
                } else {
                    out.push_str(&format!("- **File**: `{}`\n", path));
                }
            }
            if let Some(state) = &comment.state {
                out.push_str(&format!("- **Status**: {}\n", state));
            }
            out.push('\n');
            out.push_str(&comment.body);
            out.push_str("\n\n---\n\n");
        }
    }

    out
}

fn pr_last_marker(check_content: &str) -> String {
    check_content
        .lines()
        .rev()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .unwrap_or_default()
}

fn parse_pr_bot_statuses(check_content: &str) -> HashMap<String, String> {
    let mut statuses = HashMap::new();
    let mut in_section = false;
    for line in check_content.lines() {
        let trimmed = line.trim();
        if trimmed == "### Per-Bot Status" {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with("### ") {
            break;
        }
        if !in_section || !trimmed.starts_with('|') {
            continue;
        }
        let columns = trimmed
            .split('|')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if columns.len() < 3 || columns[0] == "Bot" || columns[0].starts_with("---") {
            continue;
        }
        statuses.insert(columns[0].to_string(), columns[1].to_string());
    }
    statuses
}

fn count_pr_issues_found(check_content: &str) -> u32 {
    let mut in_section = false;
    let mut count = 0;
    for line in check_content.lines() {
        let trimmed = line.trim();
        if trimmed == "### Issues Found (if any)" || trimmed == "### Issues Found" {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with("### ") {
            break;
        }
        if in_section
            && (trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed
                    .chars()
                    .next()
                    .map(|ch| ch.is_ascii_digit())
                    .unwrap_or(false)
                    && trimmed.contains(". "))
        {
            count += 1;
        }
    }
    count
}

fn update_pr_goal_tracker(goal_tracker_path: &Path, round: u32, bot_results: Option<(&str, u32, u32)>) -> Result<()> {
    if !goal_tracker_path.exists() {
        return Ok(());
    }

    let original = fs::read_to_string(goal_tracker_path)?;
    let (reviewer, new_issues, new_resolved) = bot_results.unwrap_or(("Codex", 0, 0));

    let summary_pattern = format!("| {}     | {} |", round, reviewer);
    let log_pattern = format!("### Round {}\n{}:", round, reviewer);
    let has_summary_row = original.contains(&summary_pattern);
    let has_log_entry = original.contains(&log_pattern);
    if has_summary_row && has_log_entry {
        return Ok(());
    }

    let current_found = original
        .lines()
        .find_map(|line| line.strip_prefix("- Total Issues Found: "))
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(0);
    let current_resolved = original
        .lines()
        .find_map(|line| line.strip_prefix("- Total Issues Resolved: "))
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(0);
    let total_found = if has_summary_row { current_found } else { current_found + new_issues };
    let total_resolved = if has_summary_row {
        current_resolved
    } else {
        current_resolved + new_resolved
    };
    let remaining = total_found.saturating_sub(total_resolved);
    let status = if new_issues == 0 && new_resolved == 0 {
        "Approved"
    } else if new_issues > 0 {
        "Issues Found"
    } else {
        "Resolved"
    };

    let mut updated = if has_summary_row {
        original.clone()
    } else {
        original
            .replace(
                &format!(
                    "- Total Issues Found: {}\n- Total Issues Resolved: {}\n- Remaining: {}",
                    current_found, current_resolved, current_found.saturating_sub(current_resolved)
                ),
                &format!(
                    "- Total Issues Found: {}\n- Total Issues Resolved: {}\n- Remaining: {}",
                    total_found, total_resolved, remaining
                ),
            )
    };

    if !has_summary_row {
        let row = format!(
            "| {}     | {} | {}            | {}               | {} |",
            round, reviewer, new_issues, new_resolved, status
        );
        if let Some(marker) = updated.find("## Total Statistics") {
            updated.insert_str(marker, &format!("{}\n\n", row));
        }
    }

    if !has_log_entry {
        updated.push_str(&format!(
            "\n### Round {}\n{}: Found {} issues, Resolved {}\nUpdated: {}\n",
            round,
            reviewer,
            new_issues,
            new_resolved,
            now_utc_string()
        ));
    }

    fs::write(goal_tracker_path, updated)?;
    Ok(())
}

fn build_pr_feedback_markdown(
    next_round: u32,
    max_iterations: u32,
    pr_number: u32,
    loop_dir: &Path,
    active_bots: &[String],
    check_content: &str,
) -> String {
    let bot_mentions = build_bot_mention_string(active_bots);
    format!(
        "# PR Loop Feedback (Round {})\n\n## Bot Review Analysis\n\n{}\n\n---\n\n## Your Task\n\nAddress the issues identified above:\n\n1. Read and understand each issue\n2. Make the necessary code changes\n3. Commit and push your changes\n4. Comment on the PR to trigger re-review:\n   ```bash\ngh pr comment {} --body \"{} please review the latest changes\"\n   ```\n5. Write your resolution summary to: {}\n\n---\n\n**Remaining active bots:** {}\n**Round:** {} of {}\n",
        next_round,
        check_content.trim(),
        pr_number,
        bot_mentions,
        loop_dir.join(format!("round-{}-pr-resolve.md", next_round)).display(),
        active_bots.join(", "),
        next_round,
        max_iterations
    )
}

fn current_unix_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn git_stdout(project_root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(["-C", project_root.to_str().unwrap_or(".")])
        .args(args)
        .output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_stdout_optional(project_root: &Path, args: &[&str]) -> Option<String> {
    git_stdout(project_root, args).ok().filter(|value| !value.is_empty())
}

fn pr_ahead_count(project_root: &Path, repo: &str, pr_number: u32) -> Result<u32> {
    if let Some(status) = git_stdout_optional(project_root, &["status", "-sb"]) {
        if let Some(idx) = status.find("ahead ") {
            let number = status[idx + 6..]
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>();
            if let Ok(count) = number.parse::<u32>() {
                return Ok(count);
            }
        }
    }

    let current_branch = git_current_branch(project_root).map_err(|_| anyhow::anyhow!("git branch lookup failed"))?;
    let local_head = git_stdout_optional(project_root, &["rev-parse", "HEAD"]).unwrap_or_default();

    if git_stdout(project_root, &["rev-parse", "--abbrev-ref", "@{u}"]).is_err() {
        let remote_ref = format!("origin/{}", current_branch);
        let remote_head = git_stdout_optional(project_root, &["rev-parse", &remote_ref]);
        if let Some(remote_head) = remote_head {
            if !local_head.is_empty() && local_head != remote_head {
                let count = git_stdout_optional(project_root, &["rev-list", "--count", &format!("{}..HEAD", remote_ref)])
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0);
                return Ok(count);
            }
        } else {
            let commit_info = gh_pr_commit_info_in_repo(project_root, repo, pr_number)?;
            if !commit_info.latest_commit_sha.is_empty() && local_head != commit_info.latest_commit_sha {
                let count = git_stdout_optional(
                    project_root,
                    &["rev-list", "--count", &format!("{}..HEAD", commit_info.latest_commit_sha)],
                )
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(1);
                return Ok(count.max(1));
            }
        }
        return Ok(0);
    }

    Ok(git_stdout_optional(project_root, &["rev-list", "--count", "@{u}..HEAD"])
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0))
}

fn pr_poll_reviews(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
    configured_bots: &[String],
    current_active_bots: &[String],
    poll_interval_secs: u64,
    poll_timeout_secs: u64,
    after_timestamp: &str,
    timeout_anchor_epoch: i64,
    state_started_at: Option<&str>,
    loop_dir: &Path,
) -> Result<PrPollOutcome> {
    let mut responded_bots = HashSet::new();
    let mut timed_out_bots = HashSet::new();
    let mut seen_comment_ids = HashSet::new();
    let mut comments = Vec::new();
    let mut active_bots = current_active_bots.to_vec();

    loop {
        let now = current_unix_epoch();
        let mut waiting_bots = Vec::new();

        for bot in configured_bots {
            if responded_bots.contains(bot) || timed_out_bots.contains(bot) {
                continue;
            }
            if now - timeout_anchor_epoch >= poll_timeout_secs as i64 {
                timed_out_bots.insert(bot.clone());
            } else {
                waiting_bots.push(bot.clone());
            }
        }

        if waiting_bots.is_empty() {
            break;
        }

        if loop_dir.join(".cancel-requested").exists() {
            break;
        }

        let events = gh_fetch_review_events(project_root, repo, pr_number)
            .map(|events| filter_review_events(events, after_timestamp, &waiting_bots))
            .unwrap_or_default();

        for event in events {
            if !seen_comment_ids.insert(event.id) {
                continue;
            }
            if let Some(bot) = map_author_to_bot(&event.author) {
                if configured_bots.contains(&bot) {
                    responded_bots.insert(bot);
                }
            }
            comments.push(event);
        }

        if !responded_bots.contains("codex") && configured_bots.iter().any(|bot| bot == "codex") {
            let reaction_after = after_timestamp
                .is_empty()
                .then_some(state_started_at.unwrap_or(""))
                .unwrap_or(after_timestamp);
            if gh_find_codex_thumbsup(project_root, repo, pr_number, Some(reaction_after))?.is_some() {
                responded_bots.insert("codex".to_string());
                active_bots.retain(|bot| bot != "codex");
            }
        }

        let all_done = configured_bots
            .iter()
            .all(|bot| responded_bots.contains(bot) || timed_out_bots.contains(bot));
        if all_done {
            break;
        }

        sleep(Duration::from_secs(poll_interval_secs.max(1)));
    }

    Ok(PrPollOutcome {
        comments,
        timed_out_bots,
        active_bots,
    })
}

fn run_pr_codex_review(
    project_root: &Path,
    loop_dir: &Path,
    state: &humanize_core::state::State,
    next_round: u32,
    configured_bots: &[String],
    comment_file: &Path,
    check_file: &Path,
) -> Result<String> {
    let expected_bots = configured_bots
        .iter()
        .map(|bot| format!("- {}", bot))
        .collect::<Vec<_>>()
        .join("\n");
    let comments = fs::read_to_string(comment_file).unwrap_or_default();
    let goal_tracker_file = loop_dir.join("goal-tracker.md");
    let prompt = format!(
        "# PR Review Validation (Per-Bot Analysis)\n\nAnalyze the following bot reviews and determine approval status FOR EACH BOT.\n\n## Expected Bots\n{}\n\n## Bot Reviews\n{}\n\n## Your Task\n\n1. For EACH expected bot, analyze their review (if present)\n2. Determine if each bot is:\n   - **APPROVE**: Bot explicitly approves or says \"no issues found\", \"LGTM\", \"Didn't find any major issues\", etc.\n   - **ISSUES**: Bot identifies specific problems that need fixing\n   - **NO_RESPONSE**: Bot did not post any new comments\n\n3. Output your analysis with this EXACT structure:\n\n### Per-Bot Status\n| Bot | Status | Summary |\n|-----|--------|---------|\n| <bot_name> | APPROVE/ISSUES/NO_RESPONSE | <brief summary> |\n\n### Issues Found (if any)\nList ALL specific issues from bots that have ISSUES status.\n\n### Approved Bots (to remove from active_bots)\nList bots that should be removed from active tracking (those with APPROVE status).\n\n### Final Recommendation\n- If ALL bots have APPROVE status: End with \"APPROVE\" on its own line\n- If any bot has ISSUES status: End with \"ISSUES_REMAINING\" on its own line\n- If any bot has NO_RESPONSE status: End with \"WAITING_FOR_BOTS\" on its own line\n- If any bot response indicates usage/rate limits hit (e.g. \"usage limits\", \"rate limit\", \"quota exceeded\"): End with \"USAGE_LIMIT_HIT\" on its own line\n\nAfter analysis, update the goal tracker at {} with current status.\n",
        expected_bots,
        comments,
        goal_tracker_file.display()
    );

    let prompt_file = loop_dir.join(format!("round-{}-codex-prompt.md", next_round));
    fs::write(&prompt_file, &prompt)?;

    let mut options = humanize_core::codex::CodexOptions::from_env(project_root);
    options.model = state.codex_model.clone();
    options.effort = state.codex_effort.clone();
    options.timeout_secs = state.codex_timeout;

    let result = humanize_core::codex::run_exec(&prompt, &options).map_err(|err| {
        anyhow::anyhow!("Codex failed to validate bot reviews: {}", err)
    })?;
    fs::write(check_file, &result.stdout)?;
    Ok(result.stdout)
}

fn handle_stop_pr() -> Result<()> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;
    let _input: StopHookInput = serde_json::from_str(&raw).unwrap_or_default();

    let project_root = resolve_project_root()?;
    let loop_base_dir = project_root.join(".humanize/pr-loop");
    let Some(loop_dir) = newest_active_pr_loop(&loop_base_dir) else {
        return Ok(());
    };
    let state_file = loop_dir.join("state.md");
    if !state_file.exists() {
        return Ok(());
    }

    ensure_command_exists("gh", "Error: PR loop requires GitHub CLI (gh)")?;

    let plugin_root = resolve_plugin_root()?;
    let template_root = humanize_core::template::template_dir(&plugin_root);

    let state_content = fs::read_to_string(&state_file)
        .context("Malformed PR loop state file, blocking operation for safety")?;
    let mut state = humanize_core::state::State::from_markdown(&state_content)
        .map_err(|_| anyhow::anyhow!("Malformed PR loop state file, blocking operation for safety"))?;

    let pr_number = state
        .pr_number
        .ok_or_else(|| anyhow::anyhow!("Malformed PR state: missing pr_number"))?;
    let configured_bots = sanitize_bot_list(
        &state
            .configured_bots
            .clone()
            .or_else(|| state.active_bots.clone())
            .unwrap_or_default(),
    );
    if configured_bots.is_empty() {
        return emit_stop_block(
            "Malformed PR state: configured_bots is empty.",
            Some("PR Loop: Blocked - malformed state"),
        );
    }
    let mut active_bots = sanitize_bot_list(
        &state
            .active_bots
            .clone()
            .unwrap_or_else(|| configured_bots.clone()),
    );
    state.configured_bots = Some(configured_bots.clone());
    state.active_bots = Some(active_bots.clone());

    let pr_repo = gh_resolve_pr_repo(&project_root, pr_number)?;
    let pr_state = gh_pr_state_in_repo(&project_root, &pr_repo, pr_number)?;
    if pr_state == "MERGED" {
        humanize_core::state::State::rename_to_terminal(&state_file, "merged")?;
        return Ok(());
    }
    if pr_state == "CLOSED" {
        humanize_core::state::State::rename_to_terminal(&state_file, "closed")?;
        return Ok(());
    }

    let resolve_file = loop_dir.join(format!("round-{}-pr-resolve.md", state.current_round));
    if !resolve_file.exists() {
        return emit_stop_block(
            &format!(
                "# Resolution Summary Missing\n\nPlease write your resolution summary to: {}\n\nThe summary should include:\n- Issues addressed\n- Files modified\n- Tests added (if any)",
                resolve_file.display()
            ),
            Some("PR Loop: Resolution summary missing"),
        );
    }

    let git_status = git_stdout(&project_root, &["status", "--porcelain"]).map_err(|_| {
        anyhow::anyhow!("Git status operation failed. Please check your repository state and try again.")
    })?;
    let large_files = detect_large_changed_files(&project_root, &git_status);
    if !large_files.is_empty() {
        return emit_stop_block(
            &build_large_files_reason(&large_files),
            Some("PR Loop: Large files detected"),
        );
    }
    let non_humanize_status = git_status
        .lines()
        .filter(|line| !line.contains(".humanize"))
        .collect::<Vec<_>>();
    if !non_humanize_status.is_empty() {
        return emit_stop_block(
            &format!(
                "# Git Not Clean\n\nYou have uncommitted changes. Please commit all changes before exiting.\n\nChanges detected:\n```\n{}\n```",
                non_humanize_status.join("\n")
            ),
            Some("PR Loop: Uncommitted changes detected"),
        );
    }

    let ahead_count = pr_ahead_count(&project_root, &pr_repo, pr_number)?;
    if ahead_count > 0 {
        let current_branch = git_current_branch(&project_root).unwrap_or_else(|_| "main".to_string());
        let reason = render_safe_template(
            &template_root,
            "block/unpushed-commits.md",
            "# Unpushed Commits Detected\n\nYou have {{AHEAD_COUNT}} unpushed commit(s). PR loop requires pushing changes so bots can review them.\n\nPlease push: git push origin {{CURRENT_BRANCH}}",
            &[
                ("AHEAD_COUNT", ahead_count.to_string()),
                ("CURRENT_BRANCH", current_branch),
            ],
        );
        return emit_stop_block(&reason, Some("PR Loop: Unpushed commits detected"));
    }

    let current_head = humanize_core::git::get_head_sha(&project_root)
        .map_err(|_| anyhow::anyhow!("Failed to determine current HEAD SHA"))?;
    if let Some(previous_sha) = state.latest_commit_sha.clone() {
        if !previous_sha.is_empty()
            && previous_sha != current_head
            && !humanize_core::git::is_ancestor(&project_root, &previous_sha, &current_head).unwrap_or(false)
        {
            let commit_info = gh_pr_commit_info_in_repo(&project_root, &pr_repo, pr_number)
                .unwrap_or(PrCommitInfo {
                    latest_commit_sha: current_head.clone(),
                    latest_commit_at: now_utc_string(),
                });
            state.latest_commit_sha = Some(current_head.clone());
            state.latest_commit_at = Some(commit_info.latest_commit_at.clone());
            state.last_trigger_at = None;
            state.trigger_comment_id = None;
            state.save(&state_file)?;

            let reason = render_safe_template(
                &template_root,
                "block/force-push-detected.md",
                "# Force Push Detected\n\nA force push (history rewrite) has been detected. Post a new @bot trigger comment: {{BOT_MENTION_STRING}}",
                &[
                    ("OLD_COMMIT", previous_sha),
                    ("NEW_COMMIT", current_head.clone()),
                    ("BOT_MENTION_STRING", build_bot_mention_string(&configured_bots)),
                    ("PR_NUMBER", pr_number.to_string()),
                ],
            );
            return emit_stop_block(
                &reason,
                Some("PR Loop: Force push detected - please re-trigger bots"),
            );
        }
    }

    let next_round = state.current_round + 1;
    if next_round > state.max_iterations {
        humanize_core::state::State::rename_to_terminal(&state_file, "maxiter")?;
        return Ok(());
    }

    if active_bots.is_empty() {
        humanize_core::state::State::rename_to_terminal(&state_file, "approve")?;
        return Ok(());
    }

    let current_user = gh_current_user(&project_root).ok();
    let commit_info = gh_pr_commit_info_in_repo(&project_root, &pr_repo, pr_number)?;
    let mut new_commits_detected = false;
    if !commit_info.latest_commit_at.is_empty()
        && state
            .latest_commit_at
            .as_deref()
            .map(|existing| existing != commit_info.latest_commit_at)
            .unwrap_or(true)
    {
        state.latest_commit_at = Some(commit_info.latest_commit_at.clone());
        state.last_trigger_at = None;
        state.trigger_comment_id = None;
        state.save(&state_file)?;
        new_commits_detected = true;
    }

    if let Some(user) = current_user.as_deref() {
        let trigger = gh_detect_trigger_comment(
            &project_root,
            &pr_repo,
            pr_number,
            user,
            &configured_bots,
            state.latest_commit_at.as_deref(),
        )?;
        if let Some(trigger) = trigger {
            let should_update = state
                .last_trigger_at
                .as_deref()
                .map(|current| trigger.created_at.as_str() > current)
                .unwrap_or(true);
            if should_update {
                state.last_trigger_at = Some(trigger.created_at);
                state.trigger_comment_id = Some(trigger.id.to_string());
                state.save(&state_file)?;
            }
        }
    }

    let startup_case = state.startup_case.clone().unwrap_or_else(|| "1".to_string());
    let require_trigger = pr_requires_trigger(state.current_round, &startup_case, new_commits_detected);

    if active_bots.iter().any(|bot| bot == "codex") {
        let reaction_after = state
            .last_trigger_at
            .as_deref()
            .or(state.started_at.as_deref());
        if !(require_trigger && state.last_trigger_at.is_none()) {
            if gh_find_codex_thumbsup(&project_root, &pr_repo, pr_number, reaction_after)?.is_some() {
                active_bots.retain(|bot| bot != "codex");
                state.active_bots = Some(active_bots.clone());
                state.save(&state_file)?;
                if active_bots.is_empty() {
                    humanize_core::state::State::rename_to_terminal(&state_file, "approve")?;
                    return Ok(());
                }
            }
        }
    }

    if require_trigger && state.last_trigger_at.is_none() {
        let startup_case_desc = match startup_case.as_str() {
            "4" => "New commits after all bots reviewed",
            "5" => "New commits after partial bot reviews",
            _ => "Subsequent round requires trigger",
        };
        let reason = render_safe_template(
            &template_root,
            "block/no-trigger-comment.md",
            "# Missing Trigger Comment\n\nNo @bot mention found. Please run: gh pr comment {{PR_NUMBER}} --body \"{{BOT_MENTION_STRING}} please review\"",
            &[
                ("STARTUP_CASE", startup_case.clone()),
                ("STARTUP_CASE_DESC", startup_case_desc.to_string()),
                ("CURRENT_ROUND", state.current_round.to_string()),
                ("BOT_MENTION_STRING", build_bot_mention_string(&configured_bots)),
                ("PR_NUMBER", pr_number.to_string()),
            ],
        );
        return emit_stop_block(
            &reason,
            Some("PR Loop: Missing trigger comment - please @mention bots first"),
        );
    }

    if configured_bots.iter().any(|bot| bot == "claude") && require_trigger {
        if let Some(comment_id) = state.trigger_comment_id.as_deref() {
            if gh_wait_for_claude_eyes(
                &project_root,
                &pr_repo,
                comment_id,
                3,
                Duration::from_secs(5),
            )?
            .is_none()
            {
                let reason = render_safe_template(
                    &template_root,
                    "block/claude-eyes-timeout.md",
                    "# Claude Bot Not Responding\n\nThe Claude bot did not respond with an 'eyes' reaction within 15 seconds (3 x 5s retries).\nPlease verify the Claude bot is installed and configured for this repository.",
                    &[
                        ("RETRY_COUNT", "3".to_string()),
                        ("TOTAL_WAIT_SECONDS", "15".to_string()),
                    ],
                );
                return emit_stop_block(
                    &reason,
                    Some("PR Loop: Claude bot not responding - check bot configuration"),
                );
            }
        }
    }

    let mut use_all_comments = false;
    let after_timestamp = if let Some(trigger_at) = state.last_trigger_at.clone() {
        trigger_at
    } else if state.current_round == 0 {
        match startup_case.as_str() {
            "1" => state.started_at.clone().unwrap_or_else(now_utc_string),
            "2" | "3" => {
                use_all_comments = true;
                "1970-01-01T00:00:00Z".to_string()
            }
            _ => state.started_at.clone().unwrap_or_else(now_utc_string),
        }
    } else {
        return emit_stop_block(
            &format!(
                "# Missing Trigger Comment\n\nNo @bot mention comment found from you on this PR.\n\nBefore polling for bot reviews, you must comment on the PR to trigger the bots.\n\n```bash\ngh pr comment {} --body \"{} please review the latest changes\"\n```",
                pr_number,
                build_bot_mention_string(&configured_bots)
            ),
            Some("PR Loop: Missing trigger comment"),
        );
    };

    let timeout_anchor_epoch = if use_all_comments {
        current_unix_epoch()
    } else {
        parse_iso_timestamp_epoch(&after_timestamp).unwrap_or_else(current_unix_epoch)
    };

    let poll_outcome = pr_poll_reviews(
        &project_root,
        &pr_repo,
        pr_number,
        &configured_bots,
        &active_bots,
        state.poll_interval.unwrap_or(30),
        state.poll_timeout.unwrap_or(900),
        &after_timestamp,
        timeout_anchor_epoch,
        state.started_at.as_deref(),
        &loop_dir,
    )?;
    active_bots = poll_outcome.active_bots.clone();

    if poll_outcome.comments.is_empty() {
        let timed_out = poll_outcome.timed_out_bots;
        if !timed_out.is_empty() {
            active_bots.retain(|bot| !timed_out.contains(bot));
            state.active_bots = Some(active_bots.clone());
            state.save(&state_file)?;
        }

        if active_bots.is_empty() {
            humanize_core::state::State::rename_to_terminal(&state_file, "approve")?;
            return Ok(());
        }

        return emit_stop_block(
            &format!(
                "# Bot Review Timeout\n\nNo new reviews received from bots after polling.\n\n**Bots that did not respond:** {}\n\nThis might mean:\n- The bots haven't been triggered\n- The bots are slow to respond\n- The bots are not enabled on this repository",
                active_bots.join(", ")
            ),
            Some("PR Loop: Bot review timeout"),
        );
    }

    ensure_command_exists("codex", "Error: PR loop requires codex to run")?;

    let comment_file = loop_dir.join(format!("round-{}-pr-comment.md", next_round));
    fs::write(
        &comment_file,
        format_pr_review_comments_markdown(
            next_round,
            &configured_bots,
            &active_bots,
            &poll_outcome.comments,
        ),
    )?;

    let check_file = loop_dir.join(format!("round-{}-pr-check.md", next_round));
    let check_content = run_pr_codex_review(
        &project_root,
        &loop_dir,
        &state,
        next_round,
        &configured_bots,
        &comment_file,
        &check_file,
    )?;
    let last_marker = pr_last_marker(&check_content);

    if last_marker == "APPROVE" {
        update_pr_goal_tracker(
            &loop_dir.join("goal-tracker.md"),
            next_round,
            Some(("All", 0, 0)),
        )?;
        humanize_core::state::State::rename_to_terminal(&state_file, "approve")?;
        return Ok(());
    }

    if last_marker == "WAITING_FOR_BOTS" {
        return emit_stop_block(
            "# Waiting for Bot Responses\n\nSome bots haven't posted their reviews yet.\n\nWait and try exiting again, or comment on the PR to trigger bot reviews.",
            Some("PR Loop: Waiting for bot responses"),
        );
    }

    if last_marker == "USAGE_LIMIT_HIT" {
        fs::rename(&state_file, loop_dir.join("usage-limit-state.md"))?;
        return Ok(());
    }

    let bot_statuses = parse_pr_bot_statuses(&check_content);
    let timed_out_bots = poll_outcome.timed_out_bots;
    let mut new_active_bots = Vec::new();
    for bot in &configured_bots {
        if timed_out_bots.contains(bot) {
            continue;
        }
        match bot_statuses.get(bot).map(String::as_str) {
            Some("ISSUES") => new_active_bots.push(bot.clone()),
            Some("APPROVE") => {}
            Some("NO_RESPONSE") => {
                if active_bots.contains(bot) {
                    new_active_bots.push(bot.clone());
                }
            }
            _ => {
                if active_bots.contains(bot) {
                    new_active_bots.push(bot.clone());
                }
            }
        }
    }

    update_pr_goal_tracker(
        &loop_dir.join("goal-tracker.md"),
        next_round,
        Some(("codex", count_pr_issues_found(&check_content), 0)),
    )?;

    let refreshed_commit_info = gh_pr_commit_info_in_repo(&project_root, &pr_repo, pr_number)
        .unwrap_or(PrCommitInfo {
            latest_commit_sha: current_head.clone(),
            latest_commit_at: state
                .latest_commit_at
                .clone()
                .unwrap_or_else(now_utc_string),
        });
    let startup = gh_startup_case(
        &project_root,
        &pr_repo,
        pr_number,
        &configured_bots,
        &refreshed_commit_info.latest_commit_at,
    )?;

    state.current_round = next_round;
    state.active_bots = Some(new_active_bots.clone());
    state.latest_commit_sha = Some(current_head);
    state.latest_commit_at = Some(refreshed_commit_info.latest_commit_at);
    state.startup_case = Some(startup.case_num.to_string());
    state.last_trigger_at = None;
    state.save(&state_file)?;

    if new_active_bots.is_empty() {
        humanize_core::state::State::rename_to_terminal(&state_file, "approve")?;
        return Ok(());
    }

    let feedback_file = loop_dir.join(format!("round-{}-pr-feedback.md", next_round));
    let feedback = build_pr_feedback_markdown(
        next_round,
        state.max_iterations,
        pr_number,
        &loop_dir,
        &new_active_bots,
        &check_content,
    );
    fs::write(&feedback_file, &feedback)?;
    emit_stop_block(
        &feedback,
        Some(&format!(
            "PR Loop: Round {}/{} - Bot reviews identified issues",
            next_round, state.max_iterations
        )),
    )
}

fn parse_model_and_effort(spec: &str, default_effort: &str) -> (String, String) {
    match spec.split_once(':') {
        Some((model, effort)) => (model.to_string(), effort.to_string()),
        None => (spec.to_string(), default_effort.to_string()),
    }
}

struct ValidatedPlan {
    full_path: PathBuf,
    line_count: usize,
}

fn validate_setup_plan_file(
    project_root: &Path,
    plan_file: &str,
    track_plan_file: bool,
) -> Result<ValidatedPlan> {
    if Path::new(plan_file).is_absolute() {
        bail!("Error: Plan file must be a relative path, got: {}", plan_file);
    }
    if plan_file.chars().any(char::is_whitespace) {
        bail!("Error: Plan file path cannot contain spaces");
    }
    if plan_file
        .chars()
        .any(|c| matches!(c, ';' | '&' | '|' | '$' | '`' | '<' | '>' | '(' | ')' | '{' | '}' | '[' | ']' | '!' | '#' | '~' | '*' | '?' | '\\'))
    {
        bail!("Error: Plan file path contains shell metacharacters");
    }

    humanize_core::fs::validate_path(
        plan_file,
        &humanize_core::fs::PathValidationOptions {
            allow_symlinks: false,
            allow_absolute: false,
            allow_parent_traversal: false,
            repo_root: Some(project_root.to_path_buf()),
        },
    )
    .map_err(|e| anyhow::anyhow!("Error: {}", e))?;

    let full_path = project_root.join(plan_file);
    let metadata = fs::symlink_metadata(&full_path)
        .with_context(|| format!("Error: Plan file not found: {}", plan_file))?;
    if metadata.file_type().is_symlink() {
        bail!("Error: Plan file cannot be a symbolic link");
    }
    if !metadata.is_file() {
        bail!("Error: Plan file not found: {}", plan_file);
    }

    let content = fs::read_to_string(&full_path)
        .with_context(|| format!("Error: Plan file not readable: {}", plan_file))?;
    let line_count = content.lines().count();
    if line_count < 5 {
        bail!(
            "Error: Plan is too simple (only {} lines, need at least 5)",
            line_count
        );
    }
    let content_lines = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && !(trimmed.starts_with("<!--") && trimmed.ends_with("-->"))
        })
        .count();
    if content_lines < 3 {
        bail!(
            "Error: Plan file has insufficient content (only {} content lines)",
            content_lines
        );
    }

    let tracked = git_path_is_tracked(project_root, plan_file)
        .map_err(|e| anyhow::anyhow!("Error: {:?}", e))?;
    let plan_status = git_path_status_porcelain(project_root, plan_file)
        .map_err(|e| anyhow::anyhow!("Error: {:?}", e))?;
    if track_plan_file {
        if !tracked {
            bail!("Error: --track-plan-file requires plan file to be tracked in git");
        }
        if !plan_status.trim().is_empty() {
            bail!("Error: --track-plan-file requires plan file to be clean (no modifications)");
        }
    } else if tracked {
        bail!("Error: Plan file must be gitignored when not using --track-plan-file");
    }

    Ok(ValidatedPlan {
        full_path,
        line_count,
    })
}

fn detect_base_branch(project_root: &Path, requested: Option<&str>) -> Result<String> {
    if let Some(branch) = requested {
        if local_branch_exists(project_root, branch)? {
            return Ok(branch.to_string());
        }
        bail!("Error: Specified base branch does not exist: {}", branch);
    }

    if let Some(remote_default) = git_remote_default_branch(project_root)? {
        if local_branch_exists(project_root, &remote_default)? {
            return Ok(remote_default);
        }
    }
    if local_branch_exists(project_root, "main")? {
        return Ok("main".to_string());
    }
    if local_branch_exists(project_root, "master")? {
        return Ok("master".to_string());
    }

    bail!("Error: Cannot determine base branch for code review");
}

fn local_branch_exists(project_root: &Path, branch: &str) -> Result<bool> {
    let status = Command::new("git")
        .args(["-C", project_root.to_str().unwrap_or(".")])
        .args(["show-ref", "--verify", "--quiet", &format!("refs/heads/{}", branch)])
        .status()?;
    Ok(status.success())
}

fn git_remote_default_branch(project_root: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["-C", project_root.to_str().unwrap_or(".")])
        .args(["remote", "show", "origin"])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(branch) = line.trim().strip_prefix("HEAD branch:") {
            let branch = branch.trim();
            if !branch.is_empty() && branch != "(unknown)" {
                return Ok(Some(branch.to_string()));
            }
        }
    }
    Ok(None)
}

fn git_rev_parse(project_root: &Path, rev: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["-C", project_root.to_str().unwrap_or(".")])
        .args(["rev-parse", rev])
        .output()?;
    if !output.status.success() {
        bail!("Error: Failed to get commit SHA for {}", rev);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn create_plan_backup(source: &Path, target: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)?;
    }
    #[cfg(not(unix))]
    {
        fs::copy(source, target)?;
    }
    Ok(())
}

fn skip_impl_plan_placeholder() -> &'static str {
    "# Skip Implementation Mode\n\nThis RLCR loop was started with `--skip-impl` flag, which skips the implementation phase and goes directly to code review.\n"
}

fn skip_impl_goal_tracker() -> &'static str {
    "# Goal Tracker (Skip Implementation Mode)\n\nThis RLCR loop was started with `--skip-impl` flag. The implementation phase was skipped, and the loop is running in code review mode only.\n"
}

fn skip_impl_round_prompt(summary_path: &Path, base_branch: &str, start_branch: &str) -> String {
    format!(
        "# Skip Implementation Mode - Code Review Loop\n\nThis RLCR loop was started with `--skip-impl` flag.\n\n**Mode**: Code Review Only (skipping implementation phase)\n**Base Branch**: {}\n**Current Branch**: {}\n\nWhen you're ready for review, write a brief summary of your changes and try to exit.\n\nWrite your summary to: @{}\n",
        base_branch,
        start_branch,
        summary_path.display()
    )
}

fn build_goal_tracker(plan_content: &str, plan_file: &str) -> String {
    let goal = extract_section(plan_content, &["goal", "objective", "purpose"])
        .unwrap_or_else(|| format!("[To be extracted from plan by Claude in Round 0]\n\nSource plan: {}", plan_file));
    let acceptance = extract_section(plan_content, &["acceptance", "criteria", "requirements"])
        .unwrap_or_else(|| "[To be defined by Claude in Round 0 based on the plan]".to_string());

    format!(
        "# Goal Tracker\n\n## IMMUTABLE SECTION\n\n### Ultimate Goal\n{}\n\n### Acceptance Criteria\n{}\n\n---\n\n## MUTABLE SECTION\n\n### Plan Version: 1 (Updated: Round 0)\n\n#### Plan Evolution Log\n| Round | Change | Reason | Impact on AC |\n|-------|--------|--------|--------------|\n| 0 | Initial plan | - | - |\n\n#### Active Tasks\n| Task | Target AC | Status | Notes |\n|------|-----------|--------|-------|\n| [To be populated by Claude based on plan] | - | pending | - |\n\n### Completed and Verified\n| AC | Task | Completed Round | Verified Round | Evidence |\n|----|------|-----------------|----------------|----------|\n\n### Explicitly Deferred\n| Task | Original AC | Deferred Since | Justification | When to Reconsider |\n|------|-------------|----------------|---------------|-------------------|\n\n### Open Issues\n| Issue | Discovered Round | Blocking AC | Resolution Path |\n|-------|-----------------|-------------|-----------------|\n",
        goal.trim_end(),
        acceptance.trim_end()
    )
}

fn extract_section(plan_content: &str, names: &[&str]) -> Option<String> {
    let lines: Vec<&str> = plan_content.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(header) = trimmed.strip_prefix("## ") {
            let header_lower = header.to_lowercase();
            if names.iter().any(|name| header_lower.starts_with(name)) {
                let mut section = Vec::new();
                for candidate in lines.iter().skip(idx + 1) {
                    if candidate.trim().starts_with("## ") {
                        break;
                    }
                    section.push(*candidate);
                }
                let joined = section.join("\n").trim().to_string();
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
        }
    }
    None
}

fn build_round_0_prompt(
    plan_backup_path: &Path,
    goal_tracker_path: &Path,
    summary_path: &Path,
    push_every_round: bool,
    agent_teams: bool,
) -> Result<String> {
    let mut prompt = String::from(
        "Read and execute below with ultrathink\n\n## Goal Tracker Setup (REQUIRED FIRST STEP)\n\nBefore starting implementation, you MUST initialize the Goal Tracker:\n\n1. Read @GOAL_TRACKER\n2. If the \"Ultimate Goal\" section says \"[To be extracted...]\", extract a clear goal statement from the plan\n3. If the \"Acceptance Criteria\" section says \"[To be defined...]\", define 3-7 specific, testable criteria\n4. Populate the \"Active Tasks\" table with tasks from the plan, mapping each to an AC\n5. Write the updated goal-tracker.md\n\n---\n\n## Implementation Plan\n\nFor all tasks that need to be completed, please use the Task system (TaskCreate, TaskUpdate, TaskList) to track each item in order of importance.\n\n",
    );
    prompt = prompt.replace("@GOAL_TRACKER", &goal_tracker_path.display().to_string());
    prompt.push_str(&fs::read_to_string(plan_backup_path)?);
    if agent_teams {
        prompt.push_str(
            "\n\n## Agent Teams Mode\n\nYou are operating in Agent Teams mode as the Team Leader. Split tasks into independent units and delegate all coding work.\n",
        );
    }
    prompt.push_str(&format!(
        "\n\n---\n\nAfter completing the work, please:\n1. Finalize @{}\n2. Commit your changes with a descriptive commit message\n3. Write your work summary into @{}\n",
        goal_tracker_path.display(),
        summary_path.display()
    ));
    if push_every_round {
        prompt.push_str(
            "\nNote: Since `--push-every-round` is enabled, you must push your commits to remote after each round.\n",
        );
    }
    Ok(prompt)
}

#[allow(clippy::too_many_arguments)]
fn print_setup_banner(
    plan_file: &str,
    line_count: usize,
    start_branch: &str,
    base_branch: &str,
    max_iterations: u32,
    codex_model: &str,
    codex_effort: &str,
    codex_timeout: u64,
    full_review_round: u32,
    ask_codex_question: bool,
    loop_dir: &Path,
    skip_impl: bool,
) -> Result<()> {
    if skip_impl {
        println!("=== start-rlcr-loop activated (SKIP-IMPL MODE) ===\n");
        println!("Mode: Code Review Only (--skip-impl)");
        println!("Start Branch: {}", start_branch);
        println!("Base Branch: {}", base_branch);
        println!("Codex Model: {}", codex_model);
        println!("Codex Effort: {}", codex_effort);
        println!("Codex Review Effort: high");
        println!("Codex Timeout: {}s", codex_timeout);
        println!("Loop Directory: {}\n", loop_dir.display());
    } else {
        println!("=== start-rlcr-loop activated ===\n");
        println!("Plan File: {} ({} lines)", plan_file, line_count);
        println!("Start Branch: {}", start_branch);
        println!("Base Branch: {}", base_branch);
        println!("Max Iterations: {}", max_iterations);
        println!("Codex Model: {}", codex_model);
        println!("Codex Effort: {}", codex_effort);
        println!("Codex Review Effort: high");
        println!("Codex Timeout: {}s", codex_timeout);
        println!("Full Review Round: {}", full_review_round);
        println!("Ask User for Codex Questions: {}", ask_codex_question);
        println!("Loop Directory: {}\n", loop_dir.display());
    }
    Ok(())
}

fn loop_timestamp() -> String {
    let now = chrono::Local::now();
    now.format("%Y-%m-%d_%H-%M-%S").to_string()
}

fn unique_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{}-{}-{:x}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"),
        std::process::id(),
        nanos & 0xffff
    )
}

fn sanitize_path_component(path: &Path) -> String {
    path.display()
        .to_string()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn resolve_cache_dir(project_root: &Path, unique_id: &str, skill_dir: &Path) -> Result<PathBuf> {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{}/.cache", h)))
        .unwrap_or_else(|| ".cache".to_string());

    let candidate = PathBuf::from(base)
        .join("humanize")
        .join(sanitize_path_component(project_root))
        .join(format!("skill-{}", unique_id));

    if fs::create_dir_all(&candidate).is_ok() {
        return Ok(candidate);
    }

    let fallback = skill_dir.join("cache");
    fs::create_dir_all(&fallback)?;
    Ok(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{LazyLock, Mutex};
    use tempfile::TempDir;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn plan_file_validator_allows_valid_gitignored_plan() {
        let _guard = ENV_LOCK.lock().unwrap();
        let repo = TestRepo::new();

        repo.write_file("plans/test-plan.md", "# Plan\n");
        repo.write_file(".gitignore", "plans/\n");
        repo.git(["add", ".gitignore"]);
        repo.git(["commit", "-q", "-m", "ignore plans"]);
        repo.write_active_state(&format!(
            "---\ncurrent_round: 0\nmax_iterations: 42\nplan_file: \"plans/test-plan.md\"\nplan_tracked: false\nstart_branch: {}\nbase_branch: {}\nreview_started: false\n---\n",
            repo.branch(),
            repo.branch()
        ));

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        let result = validate_plan_file(&HookInput {
            tool_name: String::new(),
            tool_input: json!(null),
            session_id: None,
            tool_output: None,
            tool_result: None,
        });
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };

        assert_eq!(result.decision, "allow");
        assert!(result.reason.is_none());
    }

    #[test]
    fn plan_file_validator_blocks_outdated_schema() {
        let _guard = ENV_LOCK.lock().unwrap();
        let repo = TestRepo::new();

        repo.write_file("plans/test-plan.md", "# Plan\n");
        repo.write_active_state(&format!(
            "---\ncurrent_round: 0\nmax_iterations: 42\nplan_file: \"plans/test-plan.md\"\nstart_branch: {}\nbase_branch: {}\nreview_started: false\n---\n",
            repo.branch(),
            repo.branch()
        ));

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        let result = validate_plan_file(&HookInput {
            tool_name: String::new(),
            tool_input: json!({}),
            session_id: None,
            tool_output: None,
            tool_result: None,
        });
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };

        assert_eq!(result.decision, "block");
        assert!(result.reason.unwrap().contains("plan_tracked"));
    }

    #[test]
    fn plan_file_validator_blocks_branch_change() {
        let _guard = ENV_LOCK.lock().unwrap();
        let repo = TestRepo::new();

        repo.write_file("plans/test-plan.md", "# Plan\n");
        repo.write_file(".gitignore", "plans/\n");
        repo.git(["add", ".gitignore"]);
        repo.git(["commit", "-q", "-m", "ignore plans"]);
        repo.write_active_state(
            "---\ncurrent_round: 0\nmax_iterations: 42\nplan_file: \"plans/test-plan.md\"\nplan_tracked: false\nstart_branch: different-branch\nbase_branch: different-branch\nreview_started: false\n---\n",
        );

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        let result = validate_plan_file(&HookInput {
            tool_name: String::new(),
            tool_input: json!({}),
            session_id: None,
            tool_output: None,
            tool_result: None,
        });
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };

        assert_eq!(result.decision, "block");
        assert!(result.reason.unwrap().contains("Git branch has changed"));
    }

    #[test]
    fn plan_file_validator_blocks_modified_tracked_plan() {
        let _guard = ENV_LOCK.lock().unwrap();
        let repo = TestRepo::new();

        repo.write_file("tracked-plan.md", "# Plan\n");
        repo.git(["add", "tracked-plan.md"]);
        repo.git(["commit", "-q", "-m", "add plan"]);
        repo.write_active_state(&format!(
            "---\ncurrent_round: 0\nmax_iterations: 42\nplan_file: tracked-plan.md\nplan_tracked: true\nstart_branch: {}\nbase_branch: {}\nreview_started: false\n---\n",
            repo.branch(),
            repo.branch()
        ));
        repo.write_file("tracked-plan.md", "# Plan\nmodified\n");

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        let result = validate_plan_file(&HookInput {
            tool_name: String::new(),
            tool_input: json!({}),
            session_id: None,
            tool_output: None,
            tool_result: None,
        });
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };

        assert_eq!(result.decision, "block");
        assert!(result.reason.unwrap().contains("uncommitted modifications"));
    }

    #[test]
    fn write_validator_blocks_pr_loop_readonly_files() {
        let input = HookInput {
            tool_name: "Write".to_string(),
            tool_input: json!({
                "file_path": "/tmp/project/.humanize/pr-loop/2026-01-18_12-00-00/round-1-pr-comment.md"
            }),
            session_id: None,
            tool_output: None,
            tool_result: None,
        };

        let result = validate_write(&input);
        assert_eq!(result.decision, "block");
        assert!(result.reason.unwrap().contains("PR Loop File Write Blocked"));
    }

    #[test]
    fn edit_validator_blocks_pr_loop_readonly_files() {
        let input = HookInput {
            tool_name: "Edit".to_string(),
            tool_input: json!({
                "file_path": "/tmp/project/.humanize/pr-loop/2026-01-18_12-00-00/round-1-pr-check.md"
            }),
            session_id: None,
            tool_output: None,
            tool_result: None,
        };

        let result = validate_edit(&input);
        assert_eq!(result.decision, "block");
        assert!(result.reason.unwrap().contains("PR Loop File Write Blocked"));
    }

    #[test]
    fn bash_validator_blocks_broad_git_add_patterns() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: json!({
                "command": "git add -A"
            }),
            session_id: None,
            tool_output: None,
            tool_result: None,
        };

        let result = validate_bash(&input);
        assert_eq!(result.decision, "block");
        assert!(result.reason.unwrap().contains("git add"));
    }

    #[test]
    fn bash_validator_blocks_python_write_to_protected_file() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: json!({
                "command": "python -c \"open('.humanize/rlcr/2026-01-18_12-00-00/state.md','w').write('x')\""
            }),
            session_id: None,
            tool_output: None,
            tool_result: None,
        };

        let result = validate_bash(&input);
        assert_eq!(result.decision, "block");
        assert!(result.reason.unwrap().contains("State File Modification Blocked"));
    }

    #[test]
    fn bash_validator_blocks_exec_fd_redirection_to_protected_file() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: json!({
                "command": "exec 3>.humanize/rlcr/2026-01-18_12-00-00/state.md"
            }),
            session_id: None,
            tool_output: None,
            tool_result: None,
        };

        let result = validate_bash(&input);
        assert_eq!(result.decision, "block");
    }

    #[test]
    fn bash_validator_blocks_append_redirect_variants() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: json!({
                "command": "printf 'x' &>> .humanize/rlcr/2026-01-18_12-00-00/round-1-summary.md"
            }),
            session_id: None,
            tool_output: None,
            tool_result: None,
        };

        let result = validate_bash(&input);
        assert_eq!(result.decision, "block");
    }

    struct TestRepo {
        tempdir: TempDir,
        loop_dir: std::path::PathBuf,
        branch: String,
    }

    impl TestRepo {
        fn new() -> Self {
            let tempdir = tempfile::tempdir().unwrap();
            let root = tempdir.path();

            run(Command::new("git").args(["init", "-q"]).current_dir(root));
            run(
                Command::new("git")
                    .args(["config", "user.email", "test@example.com"])
                    .current_dir(root),
            );
            run(
                Command::new("git")
                    .args(["config", "user.name", "Test"])
                    .current_dir(root),
            );

            std::fs::write(root.join("init.txt"), "init\n").unwrap();
            run(Command::new("git").args(["add", "init.txt"]).current_dir(root));
            run(
                Command::new("git")
                    .args(["commit", "-q", "-m", "init"])
                    .current_dir(root),
            );

            let branch_output = Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(root)
                .output()
                .unwrap();
            assert!(branch_output.status.success());
            let branch = String::from_utf8_lossy(&branch_output.stdout)
                .trim()
                .to_string();

            let loop_dir = root.join(".humanize/rlcr/2026-03-17_00-00-00");
            std::fs::create_dir_all(&loop_dir).unwrap();

            Self {
                tempdir,
                loop_dir,
                branch,
            }
        }

        fn root(&self) -> &Path {
            self.tempdir.path()
        }

        fn branch(&self) -> &str {
            &self.branch
        }

        fn git<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<std::ffi::OsStr>,
        {
            run(Command::new("git").args(args).current_dir(self.root()));
        }

        fn write_file(&self, relative: &str, contents: &str) {
            let path = self.root().join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }

        fn write_active_state(&self, contents: &str) {
            std::fs::write(self.loop_dir.join("state.md"), contents).unwrap();
        }
    }

    fn run(cmd: &mut Command) {
        let status = cmd.status().unwrap();
        assert!(status.success());
    }
}

/// Check if a line represents an empty session_id field.
fn is_empty_session_id_line(line: &str) -> bool {
    matches!(line, "session_id:" | "session_id: " | "session_id: ~" | "session_id: null")
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
    let has_empty_session_id = state_content.lines().any(is_empty_session_id_line);

    if has_empty_session_id {
        // Patch state.md by replacing empty session_id with actual value
        let patched = state_content
            .lines()
            .map(|line| {
                if is_empty_session_id_line(line) {
                    format!("session_id: {}", session_id)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Write patched content atomically
        let temp_path = format!("{}.tmp.{}", state_file_path, std::process::id());
        if std::fs::write(&temp_path, patched).is_ok()
            && std::fs::rename(&temp_path, state_file_path).is_err() {
                let _ = std::fs::remove_file(&temp_path);
            }
    }

    // Remove signal file (one-shot: session_id is now recorded)
    let _ = std::fs::remove_file(&pending_file);

    HookOutput::allow()
}

/// Check if path is a protected state file.
fn is_protected_state_file(path_lower: &str) -> bool {
    // Check for state.md in .humanize/rlcr/*/ or .humanize/pr-loop/*/
    if (path_lower.contains(".humanize/rlcr/") || path_lower.contains(".humanize/pr-loop/"))
        && path_lower.ends_with("/state.md") {
            return true;
        }
    false
}

fn newest_session_dir(base_dir: &Path) -> Option<PathBuf> {
    let mut dirs = fs::read_dir(base_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.reverse();
    dirs.into_iter().next()
}

fn state_status_label(state_file: &Path) -> String {
    match state_file.file_name().and_then(|name| name.to_str()).unwrap_or_default() {
        "state.md" => "active".to_string(),
        "finalize-state.md" => "finalize".to_string(),
        "approve-state.md" => "approve".to_string(),
        "complete-state.md" => "complete".to_string(),
        "cancel-state.md" => "cancel".to_string(),
        "maxiter-state.md" => "maxiter".to_string(),
        "merged-state.md" => "merged".to_string(),
        "closed-state.md" => "closed".to_string(),
        "stop-state.md" => "stop".to_string(),
        other if other.ends_with("-state.md") => other.trim_end_matches(".md").to_string(),
        _ => "unknown".to_string(),
    }
}

fn join_list(items: &[String], empty: &str) -> String {
    if items.is_empty() {
        empty.to_string()
    } else {
        items.join(", ")
    }
}

fn truncate_one_line(input: &str, max_len: usize) -> String {
    let single = input.lines().find(|line| !line.trim().is_empty()).unwrap_or("").trim();
    if single.len() <= max_len {
        single.to_string()
    } else {
        format!("{}...", &single[..max_len.saturating_sub(3)])
    }
}

#[derive(Debug, Clone)]
struct MonitorSnapshot {
    title: String,
    status: String,
    session_path: String,
    left_fields: Vec<(String, String)>,
    right_fields: Vec<(String, String)>,
    content_title: String,
    content_path: String,
    content: String,
    footer: String,
}

#[derive(Debug, Default)]
struct MonitorUiState {
    scroll: u16,
    follow: bool,
    last_content_key: String,
}

impl MonitorUiState {
    fn new() -> Self {
        Self {
            scroll: 0,
            follow: true,
            last_content_key: String::new(),
        }
    }
}

fn humanize_cache_base_dir(project_root: &Path) -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{}/.cache", h)))
        .unwrap_or_else(|| ".cache".to_string());
    PathBuf::from(base)
        .join("humanize")
        .join(sanitize_path_component(project_root))
}

fn newest_file_by_mtime(paths: Vec<PathBuf>) -> Option<PathBuf> {
    paths.into_iter().max_by_key(|path| {
        fs::metadata(path)
            .and_then(|meta| meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    })
}

fn read_monitor_content(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|_| format!("Unable to read {}", path.display()))
}

fn kv_block_lines(fields: &[(String, String)]) -> String {
    fields
        .iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_monitor_snapshot_text(snapshot: &MonitorSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&snapshot.title);
    out.push_str("\n\n");
    out.push_str(&format!("Session: {}\n", snapshot.session_path));
    out.push_str(&format!("Status: {}\n", snapshot.status));
    for (key, value) in snapshot
        .left_fields
        .iter()
        .chain(snapshot.right_fields.iter())
    {
        out.push_str(&format!("{key}: {value}\n"));
    }
    out.push_str(&format!("Content: {} ({})\n", snapshot.content_title, snapshot.content_path));
    out.push('\n');
    out.push_str(&snapshot.content);
    if !snapshot.content.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn monitor_status_color(status: &str) -> Color {
    match status {
        "active" | "running" | "success" | "complete" | "approve" | "merged" => Color::Green,
        "finalize" | "waiting" => Color::Yellow,
        "cancel" | "closed" | "error" | "timeout" | "maxiter" | "stop" => Color::Red,
        _ => Color::Cyan,
    }
}

fn collect_dir_files(dir: &Path, allowed_suffixes: &[&str]) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && allowed_suffixes.iter().any(|suffix| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| name.ends_with(suffix))
                        .unwrap_or(false)
                })
        })
        .collect()
}

fn rlcr_monitor_snapshot(project_root: &Path) -> Result<MonitorSnapshot> {
    let base_dir = project_root.join(".humanize/rlcr");
    let Some(loop_dir) = newest_session_dir(&base_dir) else {
        return Ok(MonitorSnapshot {
            title: "Humanize RLCR Monitor".to_string(),
            status: "idle".to_string(),
            session_path: "No RLCR sessions found".to_string(),
            left_fields: Vec::new(),
            right_fields: Vec::new(),
            content_title: "No Data".to_string(),
            content_path: "N/A".to_string(),
            content: "No RLCR sessions found.".to_string(),
            footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow".to_string(),
        });
    };
    let state_file = humanize_core::state::resolve_any_state_file(&loop_dir)
        .ok_or_else(|| anyhow::anyhow!("No state file found in latest RLCR session"))?;
    let state = humanize_core::state::State::from_file(&state_file)?;
    let status = state_status_label(&state_file);
    let summary_path = if status == "finalize" || state_file.ends_with("finalize-state.md") {
        loop_dir.join("finalize-summary.md")
    } else {
        loop_dir.join(format!("round-{}-summary.md", state.current_round))
    };
    let summary_preview = fs::read_to_string(&summary_path)
        .map(|content| truncate_one_line(&content, 96))
        .unwrap_or_else(|_| "N/A".to_string());
    let cache_dir = humanize_cache_base_dir(project_root)
        .join(loop_dir.file_name().and_then(|name| name.to_str()).unwrap_or("unknown-loop"));
    let mut candidates = collect_dir_files(
        &loop_dir,
        &[
            "-prompt.md",
            "-summary.md",
            "-review-result.md",
            "-review-prompt.md",
            "finalize-summary.md",
        ],
    );
    candidates.extend(collect_dir_files(&cache_dir, &[".log", ".out", ".cmd"]));
    let content_path = newest_file_by_mtime(candidates).unwrap_or_else(|| summary_path.clone());
    let content = read_monitor_content(&content_path);

    Ok(MonitorSnapshot {
        title: "Humanize RLCR Monitor".to_string(),
        status: status.clone(),
        session_path: loop_dir.display().to_string(),
        left_fields: vec![
            ("State File".to_string(), state_file.display().to_string()),
            ("Round".to_string(), format!("{} / {}", state.current_round, state.max_iterations)),
            (
                "Plan File".to_string(),
                if state.plan_file.is_empty() {
                    "N/A".to_string()
                } else {
                    state.plan_file.clone()
                },
            ),
            (
                "Summary Preview".to_string(),
                if summary_preview.is_empty() {
                    "N/A".to_string()
                } else {
                    summary_preview
                },
            ),
        ],
        right_fields: vec![
            (
                "Start Branch".to_string(),
                if state.start_branch.is_empty() {
                    "N/A".to_string()
                } else {
                    state.start_branch.clone()
                },
            ),
            (
                "Base Branch".to_string(),
                if state.base_branch.is_empty() {
                    "N/A".to_string()
                } else {
                    state.base_branch.clone()
                },
            ),
            ("Model".to_string(), format!("{} ({})", state.codex_model, state.codex_effort)),
            ("Timeout".to_string(), format!("{}s", state.codex_timeout)),
            ("Ask Codex".to_string(), state.ask_codex_question.to_string()),
            ("Agent Teams".to_string(), state.agent_teams.to_string()),
        ],
        content_title: "Latest RLCR Artifact".to_string(),
        content_path: content_path.display().to_string(),
        content,
        footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow".to_string(),
    })
}

fn pr_monitor_snapshot(project_root: &Path) -> Result<MonitorSnapshot> {
    let base_dir = project_root.join(".humanize/pr-loop");
    let Some(loop_dir) = newest_session_dir(&base_dir) else {
        return Ok(MonitorSnapshot {
            title: "Humanize PR Monitor".to_string(),
            status: "idle".to_string(),
            session_path: "No PR loop sessions found".to_string(),
            left_fields: Vec::new(),
            right_fields: Vec::new(),
            content_title: "No Data".to_string(),
            content_path: "N/A".to_string(),
            content: "No PR loop sessions found.".to_string(),
            footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow".to_string(),
        });
    };
    let state_file = humanize_core::state::resolve_any_state_file(&loop_dir)
        .ok_or_else(|| anyhow::anyhow!("No state file found in latest PR session"))?;
    let state = humanize_core::state::State::from_file(&state_file)?;
    let configured_bots = state.configured_bots.clone().unwrap_or_default();
    let active_bots = state.active_bots.clone().unwrap_or_default();
    let resolve_path = loop_dir.join(format!("round-{}-pr-resolve.md", state.current_round));
    let comment_path = loop_dir.join(format!("round-{}-pr-comment.md", state.current_round + 1));
    let content_path = newest_file_by_mtime(collect_dir_files(
        &loop_dir,
        &[
            "-pr-feedback.md",
            "-pr-check.md",
            "-pr-comment.md",
            "-prompt.md",
            "-pr-resolve.md",
            "goal-tracker.md",
        ],
    ))
    .unwrap_or_else(|| resolve_path.clone());
    let content = read_monitor_content(&content_path);

    Ok(MonitorSnapshot {
        title: "Humanize PR Monitor".to_string(),
        status: state_status_label(&state_file),
        session_path: loop_dir.display().to_string(),
        left_fields: vec![
            ("State File".to_string(), state_file.display().to_string()),
            (
                "PR Number".to_string(),
                state.pr_number.map(|n| format!("#{n}")).unwrap_or_else(|| "N/A".to_string()),
            ),
            (
                "Branch".to_string(),
                if state.start_branch.is_empty() {
                    "N/A".to_string()
                } else {
                    state.start_branch.clone()
                },
            ),
            ("Round".to_string(), format!("{} / {}", state.current_round, state.max_iterations)),
            ("Configured Bots".to_string(), join_list(&configured_bots, "none")),
            ("Active Bots".to_string(), join_list(&active_bots, "none")),
        ],
        right_fields: vec![
            (
                "Startup Case".to_string(),
                state.startup_case.clone().unwrap_or_else(|| "N/A".to_string()),
            ),
            ("Model".to_string(), format!("{} ({})", state.codex_model, state.codex_effort)),
            ("Timeout".to_string(), format!("{}s", state.codex_timeout)),
            (
                "Poll".to_string(),
                format!(
                    "{}s / {}s",
                    state.poll_interval.unwrap_or(0),
                    state.poll_timeout.unwrap_or(0)
                ),
            ),
            (
                "Latest Commit".to_string(),
                state.latest_commit_sha.clone().unwrap_or_else(|| "N/A".to_string()),
            ),
            (
                "Last Trigger".to_string(),
                state.last_trigger_at.clone().unwrap_or_else(|| "none".to_string()),
            ),
            (
                "Trigger Comment ID".to_string(),
                state.trigger_comment_id.clone().unwrap_or_else(|| "none".to_string()),
            ),
            ("Resolve File".to_string(), resolve_path.display().to_string()),
            ("Next Comment File".to_string(), comment_path.display().to_string()),
        ],
        content_title: "Latest PR Artifact".to_string(),
        content_path: content_path.display().to_string(),
        content,
        footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow".to_string(),
    })
}

fn skill_monitor_snapshot(project_root: &Path) -> Result<MonitorSnapshot> {
    let skill_dir = project_root.join(".humanize/skill");
    if !skill_dir.is_dir() {
        return Ok(MonitorSnapshot {
            title: "Humanize Skill Monitor".to_string(),
            status: "idle".to_string(),
            session_path: "No ask-codex invocations found".to_string(),
            left_fields: Vec::new(),
            right_fields: Vec::new(),
            content_title: "No Data".to_string(),
            content_path: "N/A".to_string(),
            content: "No ask-codex skill invocations found.".to_string(),
            footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow".to_string(),
        });
    }

    let mut dirs = fs::read_dir(&skill_dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.reverse();
    if dirs.is_empty() {
        return Ok(MonitorSnapshot {
            title: "Humanize Skill Monitor".to_string(),
            status: "idle".to_string(),
            session_path: "No ask-codex invocations found".to_string(),
            left_fields: Vec::new(),
            right_fields: Vec::new(),
            content_title: "No Data".to_string(),
            content_path: "N/A".to_string(),
            content: "No ask-codex skill invocations found.".to_string(),
            footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow".to_string(),
        });
    }

    let total = dirs.len();
    let mut success = 0usize;
    let mut error = 0usize;
    let mut timeout = 0usize;
    let mut empty = 0usize;
    let mut running = 0usize;

    for dir in &dirs {
        let metadata = dir.join("metadata.md");
        if !metadata.exists() {
            running += 1;
            continue;
        }
        let state = fs::read_to_string(&metadata).unwrap_or_default();
        if state.contains("status: success") {
            success += 1;
        } else if state.contains("status: error") {
            error += 1;
        } else if state.contains("status: timeout") {
            timeout += 1;
        } else if state.contains("status: empty_response") {
            empty += 1;
        }
    }

    let latest = &dirs[0];
    let metadata = latest.join("metadata.md");
    let metadata_content = fs::read_to_string(&metadata).unwrap_or_default();
    let status = metadata_content
        .lines()
        .find_map(|line| line.strip_prefix("status: "))
        .unwrap_or(if metadata.exists() { "unknown" } else { "running" });
    let model = metadata_content
        .lines()
        .find_map(|line| line.strip_prefix("model: "))
        .unwrap_or("N/A");
    let effort = metadata_content
        .lines()
        .find_map(|line| line.strip_prefix("effort: "))
        .unwrap_or("N/A");
    let question = fs::read_to_string(latest.join("input.md"))
        .map(|content| truncate_one_line(&content, 96))
        .unwrap_or_else(|_| "N/A".to_string());
    let invocation_id = latest.file_name().and_then(|name| name.to_str()).unwrap_or("unknown");
    let cache_dir = humanize_cache_base_dir(project_root).join(format!("skill-{invocation_id}"));
    let watched_file = newest_file_by_mtime({
        let mut files = vec![
            latest.join("output.md"),
            latest.join("input.md"),
            cache_dir.join("codex-run.out"),
            cache_dir.join("codex-run.log"),
            cache_dir.join("codex-run.cmd"),
        ];
        files.retain(|path| path.is_file());
        files
    })
    .unwrap_or_else(|| latest.join("input.md"));
    let content = read_monitor_content(&watched_file);

    Ok(MonitorSnapshot {
        title: "Humanize Skill Monitor".to_string(),
        status: status.to_string(),
        session_path: latest.display().to_string(),
        left_fields: vec![
            ("Total".to_string(), total.to_string()),
            ("Success".to_string(), success.to_string()),
            ("Error".to_string(), error.to_string()),
            ("Timeout".to_string(), timeout.to_string()),
            ("Empty".to_string(), empty.to_string()),
            ("Running".to_string(), running.to_string()),
        ],
        right_fields: vec![
            ("Latest Invocation".to_string(), latest.display().to_string()),
            ("Status".to_string(), status.to_string()),
            ("Model".to_string(), format!("{model} ({effort})")),
            ("Question".to_string(), question),
        ],
        content_title: "Invocation Output".to_string(),
        content_path: watched_file.display().to_string(),
        content,
        footer: "q/esc quit | j/k or arrows scroll | g/G top/bottom | f toggle follow".to_string(),
    })
}

fn render_monitor_tui(
    snapshot: &MonitorSnapshot,
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ui_state: &mut MonitorUiState,
) -> Result<()> {
    let content_key = format!("{}:{}", snapshot.content_path, snapshot.content.len());
    if ui_state.last_content_key != content_key {
        ui_state.last_content_key = content_key;
        if ui_state.follow {
            ui_state.scroll = u16::MAX;
        }
    }

    terminal.draw(|frame| {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(10),
                Constraint::Min(8),
                Constraint::Length(2),
            ])
            .split(area);

        frame.render_widget(Clear, area);

        let header = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    &snapshot.title,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("[{}]", snapshot.status),
                    Style::default()
                        .fg(monitor_status_color(&snapshot.status))
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                &snapshot.session_path,
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title("Session"));
        frame.render_widget(header, chunks[0]);

        let info_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

        let left = Paragraph::new(kv_block_lines(&snapshot.left_fields))
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Details"));
        frame.render_widget(left, info_chunks[0]);

        let right = Paragraph::new(kv_block_lines(&snapshot.right_fields))
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Context"));
        frame.render_widget(right, info_chunks[1]);

        let content_height = chunks[2].height.saturating_sub(2);
        let total_lines = snapshot.content.lines().count() as u16;
        let max_scroll = total_lines.saturating_sub(content_height);
        if ui_state.follow || ui_state.scroll > max_scroll {
            ui_state.scroll = max_scroll;
        }

        let content = Paragraph::new(snapshot.content.as_str())
            .scroll((ui_state.scroll, 0))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(snapshot.content_title.as_str())
                    .title_bottom(Line::from(Span::styled(
                        snapshot.content_path.as_str(),
                        Style::default().fg(Color::DarkGray),
                    ))),
            );
        frame.render_widget(content, chunks[2]);

        let footer = Paragraph::new(snapshot.footer.as_str())
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL).title("Controls"));
        frame.render_widget(footer, chunks[3]);
    })?;
    Ok(())
}

fn run_monitor_loop<F>(once: bool, interval_secs: u64, mut snapshotter: F) -> Result<()>
where
    F: FnMut() -> Result<MonitorSnapshot>,
{
    if once {
        let snapshot = snapshotter()?;
        println!("{}", render_monitor_snapshot_text(&snapshot));
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut ui_state = MonitorUiState::new();
    let mut snapshot = snapshotter()?;
    let tick_rate = Duration::from_secs(interval_secs.max(1));
    let mut last_tick = Instant::now();

    let run_result: Result<()> = (|| loop {
        render_monitor_tui(&snapshot, &mut terminal, &mut ui_state)?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break Ok(()),
                    KeyCode::Down | KeyCode::Char('j') => {
                        ui_state.follow = false;
                        ui_state.scroll = ui_state.scroll.saturating_add(1);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        ui_state.follow = false;
                        ui_state.scroll = ui_state.scroll.saturating_sub(1);
                    }
                    KeyCode::PageDown => {
                        ui_state.follow = false;
                        ui_state.scroll = ui_state.scroll.saturating_add(10);
                    }
                    KeyCode::PageUp => {
                        ui_state.follow = false;
                        ui_state.scroll = ui_state.scroll.saturating_sub(10);
                    }
                    KeyCode::Char('g') => {
                        ui_state.follow = false;
                        ui_state.scroll = 0;
                    }
                    KeyCode::Char('G') | KeyCode::End => {
                        ui_state.follow = true;
                        ui_state.scroll = u16::MAX;
                    }
                    KeyCode::Char('f') => {
                        ui_state.follow = !ui_state.follow;
                        if ui_state.follow {
                            ui_state.scroll = u16::MAX;
                        }
                    }
                    KeyCode::Char('r') => {
                        snapshot = snapshotter()?;
                        last_tick = Instant::now();
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            snapshot = snapshotter()?;
            last_tick = Instant::now();
        }
    })();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    run_result
}

/// Handle monitor commands.
pub fn handle_monitor(cmd: MonitorCommands) -> Result<()> {
    let project_root = resolve_project_root()?;
    match cmd {
        MonitorCommands::Rlcr { once, interval_secs } => {
            run_monitor_loop(once, interval_secs, || rlcr_monitor_snapshot(&project_root))
        }
        MonitorCommands::Pr { once, interval_secs } => {
            run_monitor_loop(once, interval_secs, || pr_monitor_snapshot(&project_root))
        }
        MonitorCommands::Skill { once, interval_secs } => {
            run_monitor_loop(once, interval_secs, || skill_monitor_snapshot(&project_root))
        }
    }
}

/// Install the current binary into the plugin root's bin directory.
pub fn handle_install(plugin_root: Option<&str>) -> Result<()> {
    let plugin_root = match plugin_root {
        Some(path) => PathBuf::from(path),
        None => resolve_plugin_root()?,
    };
    let source_root = runtime_components_source_root()?;
    for component in ["prompt-template", "hooks", "commands", "agents", "skills", ".claude-plugin"] {
        let src = source_root.join(component);
        if !src.exists() {
            continue;
        }
        let dst = plugin_root.join(component);
        remove_dir_if_exists(&dst)?;
        copy_dir_recursive(&src, &dst)?;
    }
    if source_root.join("docs/images").is_dir() {
        remove_dir_if_exists(&plugin_root.join("docs/images"))?;
        copy_dir_recursive(&source_root.join("docs/images"), &plugin_root.join("docs/images"))?;
    }
    println!("Installed runtime assets to {}", plugin_root.display());
    println!("Ensure `humanize` is installed on PATH before using the plugin.");
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn strip_user_invocable_frontmatter(content: &str) -> String {
    let mut out = Vec::new();
    let mut in_frontmatter = false;
    let mut frontmatter_done = false;
    for line in content.lines() {
        if !frontmatter_done && line.trim() == "---" {
            in_frontmatter = !in_frontmatter;
            if !in_frontmatter {
                frontmatter_done = true;
            }
            out.push(line.to_string());
            continue;
        }
        if in_frontmatter && line.trim_start().starts_with("user-invocable:") {
            continue;
        }
        out.push(line.to_string());
    }
    let mut rendered = out.join("\n");
    if content.ends_with('\n') {
        rendered.push('\n');
    }
    rendered
}

fn rewrite_skill_content(content: &str, runtime_root: &Path) -> String {
    let runtime_root_str = runtime_root.display().to_string();
    let rendered = content.replace("{{HUMANIZE_RUNTIME_ROOT}}", &runtime_root_str);
    strip_user_invocable_frontmatter(&rendered)
}

fn sync_skill_source(skill_name: &str, src_root: &Path, dst_root: &Path, dry_run: bool) -> Result<()> {
    let src = src_root.join(skill_name);
    let dst = dst_root.join(skill_name);
    if dry_run {
        println!("DRY-RUN sync {} -> {}", src.display(), dst.display());
        return Ok(());
    }
    remove_dir_if_exists(&dst)?;
    copy_dir_recursive(&src, &dst)
}

fn hydrate_installed_skill(skill_file: &Path, runtime_root: &Path, dry_run: bool) -> Result<()> {
    if dry_run {
        println!("DRY-RUN hydrate {}", skill_file.display());
        return Ok(());
    }
    let content = fs::read_to_string(skill_file)?;
    let rendered = rewrite_skill_content(&content, runtime_root);
    fs::write(skill_file, rendered)?;
    Ok(())
}

fn default_kimi_skills_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/agents/skills"))
}

fn default_codex_skills_dir() -> Result<PathBuf> {
    if let Ok(codex_home) = std::env::var("CODEX_HOME") {
        return Ok(PathBuf::from(codex_home).join("skills"));
    }
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".codex/skills"))
}

fn runtime_components_source_root() -> Result<PathBuf> {
    resolve_plugin_root()
}

fn install_skills_into(skills_dir: &Path, dry_run: bool) -> Result<()> {
    let source_root = runtime_components_source_root()?;
    let skills_source = source_root.join("skills");
    let runtime_root = skills_dir.join("humanize");
    let skill_names = ["ask-codex", "humanize", "humanize-gen-plan", "humanize-rlcr"];

    if !skills_source.is_dir() {
        bail!("Skills source directory not found: {}", skills_source.display());
    }

    if dry_run {
        println!("DRY-RUN target skills dir: {}", skills_dir.display());
    } else {
        fs::create_dir_all(skills_dir)?;
    }

    for skill_name in &skill_names {
        sync_skill_source(skill_name, &skills_source, skills_dir, dry_run)?;
    }

    if dry_run {
        println!("DRY-RUN sync prompt-template -> {}", runtime_root.join("prompt-template").display());
    } else {
        remove_dir_if_exists(&runtime_root.join("prompt-template"))?;
        copy_dir_recursive(&source_root.join("prompt-template"), &runtime_root.join("prompt-template"))?;
    }

    for skill_name in &skill_names {
        hydrate_installed_skill(&skills_dir.join(skill_name).join("SKILL.md"), &runtime_root, dry_run)?;
    }

    println!("Skills synced to {}", skills_dir.display());
    println!("Runtime root: {}", runtime_root.display());
    println!("Ensure `humanize` is installed on PATH before using the installed skills.");
    Ok(())
}

/// Install Humanize skills for Codex/Kimi runtimes.
pub fn handle_install_skills(
    target: &str,
    skills_dir: Option<&str>,
    kimi_skills_dir: Option<&str>,
    codex_skills_dir: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let target = target.to_ascii_lowercase();
    if !matches!(target.as_str(), "kimi" | "codex" | "both") {
        bail!("--target must be one of: kimi, codex, both");
    }

    let mut kimi_dir = kimi_skills_dir
        .map(PathBuf::from)
        .unwrap_or(default_kimi_skills_dir()?);
    let mut codex_dir = codex_skills_dir
        .map(PathBuf::from)
        .unwrap_or(default_codex_skills_dir()?);

    if let Some(legacy_dir) = skills_dir {
        let legacy = PathBuf::from(legacy_dir);
        match target.as_str() {
            "kimi" => kimi_dir = legacy,
            "codex" => codex_dir = legacy,
            "both" => {
                kimi_dir = legacy.clone();
                codex_dir = legacy;
            }
            _ => {}
        }
    }

    match target.as_str() {
        "kimi" => install_skills_into(&kimi_dir, dry_run)?,
        "codex" => install_skills_into(&codex_dir, dry_run)?,
        "both" => {
            install_skills_into(&kimi_dir, dry_run)?;
            if codex_dir != kimi_dir {
                install_skills_into(&codex_dir, dry_run)?;
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}

/// Handle gate commands.
pub fn handle_gate(cmd: GateCommands) -> Result<()> {
    match cmd {
        GateCommands::Rlcr {
            session_id,
            transcript_path,
            project_root,
            json,
        } => handle_gate_rlcr(session_id.as_deref(), transcript_path.as_deref(), project_root.as_deref(), json),
    }
}

fn handle_gate_rlcr(
    session_id: Option<&str>,
    transcript_path: Option<&str>,
    project_root: Option<&str>,
    print_json: bool,
) -> Result<()> {
    let project_root = project_root
        .map(PathBuf::from)
        .unwrap_or(resolve_project_root()?);
    let input = serde_json::json!({
        "hook_event_name": "Stop",
        "stop_hook_active": false,
        "cwd": project_root.display().to_string(),
        "session_id": session_id,
        "transcript_path": transcript_path,
    });

    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let output = Command::new(&exe)
        .args(["stop", "rlcr"])
        .env("CLAUDE_PROJECT_DIR", project_root.display().to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(input.to_string().as_bytes())?;
            }
            child.wait_with_output()
        })?;

    if !output.status.success() {
        eprintln!(
            "Error: stop hook process exited with code {:?}",
            output.status.code()
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            eprintln!("{}", stderr.trim());
        }
        std::process::exit(20);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        println!("ALLOW: stop gate passed.");
        std::process::exit(0);
    }

    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("Error: stop hook returned non-JSON output");
            eprintln!("{}", stdout);
            std::process::exit(20);
        }
    };

    let decision = parsed.get("decision").and_then(|v| v.as_str()).unwrap_or_default();
    if decision == "block" {
        if print_json {
            println!("{}", stdout);
        } else {
            if let Some(system_message) = parsed.get("systemMessage").and_then(|v| v.as_str()) {
                if !system_message.is_empty() {
                    println!("BLOCK: {}", system_message);
                }
            }
            if let Some(reason) = parsed.get("reason").and_then(|v| v.as_str()) {
                if !reason.is_empty() {
                    println!("{}", reason);
                }
            }
        }
        std::process::exit(10);
    }

    eprintln!("Error: Unexpected hook decision: {}", decision);
    eprintln!("{}", stdout);
    std::process::exit(20);
}

/// Handle stop commands.
pub fn handle_stop(cmd: StopCommands) -> Result<()> {
    match cmd {
        StopCommands::Rlcr => handle_stop_rlcr(),
        StopCommands::Pr => handle_stop_pr(),
    }
}

/// Handle ask-codex command.
pub fn handle_ask_codex(prompt: &str, model: &str, effort: &str, timeout: u64) -> Result<()> {
    ask_codex_native(prompt, model, effort, timeout)
}

/// Handle gen-plan command.
pub fn handle_gen_plan(input: &str, output: &str) -> Result<()> {
    gen_plan_native(input, output)
}
