use super::pr::render_template_or_fallback;
use super::*;

/// Validate Read tool access.
///
/// Implements full parity with loop-read-validator.sh:
/// 1. Blocks todos files unless allowlisted
/// 2. For summary/prompt files: validates location, round number, and directory
pub(super) fn validate_read(input: &HookInput) -> HookOutput {
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
            Err(_) => {
                return HookOutput::block(format!(
                    "Reading todos files is not allowed: {}",
                    file_path
                ));
            }
        };

        let loop_base_dir = format!("{}/.humanize/rlcr", project_root);
        let session_id = input.session_id.as_deref();

        if let Some(loop_dir) =
            humanize_core::state::find_active_loop(std::path::Path::new(&loop_base_dir), session_id)
        {
            // Parse state to get current round
            let state_file = humanize_core::state::resolve_active_state_file(&loop_dir);
            if let Some(state_path) = state_file {
                if let Ok(content) = std::fs::read_to_string(&state_path) {
                    if let Ok(state) = humanize_core::state::State::from_markdown_strict(&content) {
                        if humanize_core::fs::is_allowlisted_file(
                            &file_path,
                            &loop_dir,
                            state.current_round,
                        ) {
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
    let is_summary = humanize_core::fs::is_round_file_type(
        &path_lower,
        humanize_core::fs::RoundFileType::Summary,
    );
    let is_prompt = humanize_core::fs::is_round_file_type(
        &path_lower,
        humanize_core::fs::RoundFileType::Prompt,
    );

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
        Err(_) => {
            return HookOutput::block(
                "Malformed state file, blocking operation for safety".to_string(),
            );
        }
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
pub(super) fn validate_write(input: &HookInput) -> HookOutput {
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
    if humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Prompt)
    {
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
    let is_summary = humanize_core::fs::is_round_file_type(
        &path_lower,
        humanize_core::fs::RoundFileType::Summary,
    );
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
        Err(_) => {
            return HookOutput::block(
                "Malformed state file, blocking operation for safety".to_string(),
            );
        }
    };

    let current_round = state.current_round;

    // Block goal-tracker after Round 0
    if is_goal_tracker && current_round > 0 {
        let summary_file = format!(
            "{}/round-{}-summary.md",
            active_loop_dir.display(),
            current_round
        );
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
        let correct_path = format!(
            "{}/round-{}-summary.md",
            active_loop_dir.display(),
            current_round
        );
        return HookOutput::block(format!(
            "Write summary to the correct path: {}",
            correct_path
        ));
    }

    // Validate round number for summary files
    if is_summary {
        if let Some(claude_round) = humanize_core::fs::extract_round_number(&file_path) {
            if claude_round != current_round {
                let correct_path = format!(
                    "{}/round-{}-summary.md",
                    active_loop_dir.display(),
                    current_round
                );
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
    "state.md",
    "finalize-state.md",
    "goal-tracker.md",
    "round-",
    "-summary.md",
    "-prompt.md",
    "-todos.md",
    "-pr-comment.md",
    "-pr-check.md",
    "-pr-feedback.md",
    "-codex-prompt.md",
];

/// Check if a command contains any protected file pattern.
fn contains_protected_pattern(cmd_lower: &str) -> Option<&'static str> {
    PROTECTED_PATTERNS
        .iter()
        .find(|p| cmd_lower.contains(*p))
        .copied()
}

fn bash_has_control_operators(cmd_lower: &str) -> bool {
    ["|", "&&", ";", "`", "$(", "||", "<(", ">(", "\n"]
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
        ">",
        ">>",
        "tee ",
        "sed -i",
        "perl -i",
        "ruby -i",
        "truncate ",
        "dd ",
        "install ",
        "mv ",
        "cp ",
        "touch ",
        "rm ",
        "python -c",
        "python3 -c",
        "node -e",
        "ed ",
        "ex ",
        "exec >",
        "xargs ",
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
pub(super) fn validate_edit(input: &HookInput) -> HookOutput {
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
    if humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Prompt)
    {
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
        Err(_) => {
            return HookOutput::block(
                "Malformed state file, blocking operation for safety".to_string(),
            );
        }
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
        let summary_file = format!(
            "{}/round-{}-summary.md",
            active_loop_dir.display(),
            current_round
        );
        return HookOutput::block(format!(
            "Editing goal-tracker.md is not allowed after Round 0. Update it in your summary: {}",
            summary_file
        ));
    }

    // Validate round number for summary files
    if humanize_core::fs::is_round_file_type(&path_lower, humanize_core::fs::RoundFileType::Summary)
    {
        if let Some(claude_round) = humanize_core::fs::extract_round_number(&file_path) {
            if claude_round != current_round {
                let correct_path = format!(
                    "{}/round-{}-summary.md",
                    active_loop_dir.display(),
                    current_round
                );
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
pub(super) fn validate_bash(input: &HookInput) -> HookOutput {
    let command = match get_command(input) {
        Some(c) => c,
        None => return HookOutput::allow(),
    };

    let cmd_lower = command.to_lowercase();
    let cmd_trimmed = command.trim();

    // Safe command patterns
    let safe_patterns = [
        "git status",
        "git log",
        "git diff",
        "git branch",
        "git rev-parse",
        "cargo build",
        "cargo check",
        "cargo test",
        "cargo clippy",
        "cargo fmt --check",
        "ls ",
        "cat ",
        "head ",
        "tail ",
        "grep ",
        "which ",
        "pwd",
        "rg ",
        "find ",
        "wc ",
        "sed -n",
        "git show",
        "git remote",
        "git ls-files",
    ];

    for pattern in &safe_patterns {
        if cmd_lower.starts_with(&pattern.to_lowercase()) && !bash_has_control_operators(&cmd_lower)
        {
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
        "rm ", "rm\t", "rmdir", "mv ", "mv\t", "cp ", "2>", "chmod", "chown", "mkdir -p", "nohup",
        "disown", "bg ", "fg ",
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
pub(super) fn validate_plan_file(input: &HookInput) -> HookOutput {
    let project_root = std::env::var("CLAUDE_PROJECT_DIR").ok().or_else(|| {
        std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    });

    let project_root = match project_root {
        Some(p) => p,
        None => {
            return HookOutput::block(
                "Could not determine project root. Please set CLAUDE_PROJECT_DIR and try again.",
            );
        }
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
        Err(_) => return HookOutput::block("Malformed state file, blocking operation for safety"),
    };

    let state = match humanize_core::state::State::from_markdown_strict(&state_content) {
        Ok(s) => s,
        Err(_) => return HookOutput::block("Malformed state file, blocking operation for safety"),
    };

    if let Some(reason) = validate_plan_state_schema(&state_content) {
        return HookOutput::block(reason);
    }

    let current_branch = match git_current_branch(Path::new(&project_root)) {
        Ok(branch) => branch,
        Err(_) => {
            return HookOutput::block(
                "Git operation failed or timed out.\n\nCannot verify branch consistency. Please check git status and try again.",
            );
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
            ));
        }
        Err(GitPathCheckError::Io(err)) => {
            return HookOutput::block(format!(
                "Git operation failed while checking plan file tracking status.\n\n{}\n\nPlease check git status and try again.",
                err
            ));
        }
    };

    if state.plan_tracked {
        if !plan_is_tracked {
            return HookOutput::block(format!(
                "Plan file is no longer tracked in git.\n\nFile: {}\n\nThis RLCR loop was started with --track-plan-file, but the plan file has been removed from git tracking.",
                state.plan_file
            ));
        }

        let plan_git_status = match git_path_status_porcelain(
            Path::new(&project_root),
            &state.plan_file,
        ) {
            Ok(status) => status,
            Err(GitPathCheckError::Failed(code)) => {
                return HookOutput::block(format!(
                    "Git operation failed while checking plan file status (exit code: {}).\n\nPlease check git status and try again.",
                    code
                ));
            }
            Err(GitPathCheckError::Io(err)) => {
                return HookOutput::block(format!(
                    "Git operation failed while checking plan file status.\n\n{}\n\nPlease check git status and try again.",
                    err
                ));
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

pub(super) fn validate_plan_state_schema(state_content: &str) -> Option<String> {
    let mapping = match extract_state_yaml_mapping(state_content) {
        Ok(m) => m,
        Err(_) => return Some("Malformed state file, blocking operation for safety".to_string()),
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
