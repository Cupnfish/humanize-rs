use super::pr::render_template_or_fallback;
use super::*;
use regex::Regex;

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
    ".humanize/rlcr/",
];

const PR_LOOP_READONLY_PATTERNS: &[&str] = &[
    "round-[0-9]+-pr-comment\\.md",
    "round-[0-9]+-prompt\\.md",
    "round-[0-9]+-codex-prompt\\.md",
    "round-[0-9]+-pr-check\\.md",
    "round-[0-9]+-pr-feedback\\.md",
];

const SAFE_BASH_PATTERNS: &[&str] = &[
    "git status",
    "git log",
    "git diff",
    "git branch",
    "git rev-parse",
    "git show",
    "git remote",
    "git ls-files",
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
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum QuoteStyle {
    None,
    Single,
    Double,
}

struct ParsedShellArg<'a> {
    value: &'a str,
    quote: QuoteStyle,
}

fn regex_is_match(pattern: &str, text: &str) -> bool {
    Regex::new(pattern)
        .map(|regex| regex.is_match(text))
        .unwrap_or(false)
}

fn split_shell_segments(command_lower: &str) -> Vec<&str> {
    Regex::new(r"(?:\|\&|\&\&|\|\||[|;])")
        .unwrap()
        .split(command_lower)
        .collect()
}

fn has_safe_command_blockers(cmd_lower: &str) -> bool {
    [
        "|&", "&&", "||", "|", ";", "`", "$(", "${", "\n", "<(", ">(", " -c ",
    ]
    .iter()
    .any(|pattern| cmd_lower.contains(pattern))
        || cmd_lower.contains('>')
        || cmd_lower.trim_end().ends_with('&')
}

fn is_safe_bash_command(cmd_lower: &str) -> bool {
    SAFE_BASH_PATTERNS
        .iter()
        .any(|pattern| cmd_lower.starts_with(pattern) && !has_safe_command_blockers(cmd_lower))
}

fn normalize_shell_path(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    while normalized.contains("/./") {
        normalized = normalized.replace("/./", "/");
    }
    normalized.to_lowercase()
}

fn parse_shell_arg(input: &str) -> Option<(ParsedShellArg<'_>, &str)> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    let mut chars = trimmed.char_indices();
    let (_, first) = chars.next()?;
    match first {
        '"' => {
            let end = trimmed[1..].find('"')? + 1;
            let value = &trimmed[1..end];
            Some((
                ParsedShellArg {
                    value,
                    quote: QuoteStyle::Double,
                },
                &trimmed[end + 1..],
            ))
        }
        '\'' => {
            let end = trimmed[1..].find('\'')? + 1;
            let value = &trimmed[1..end];
            Some((
                ParsedShellArg {
                    value,
                    quote: QuoteStyle::Single,
                },
                &trimmed[end + 1..],
            ))
        }
        _ => {
            let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            Some((
                ParsedShellArg {
                    value: &trimmed[..end],
                    quote: QuoteStyle::None,
                },
                &trimmed[end..],
            ))
        }
    }
}

fn contains_protected_pattern(cmd_lower: &str) -> Option<&'static str> {
    PROTECTED_PATTERNS
        .iter()
        .find(|pattern| cmd_lower.contains(**pattern))
        .copied()
}

fn path_is_gitignored(project_root: &Path, path: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["check-ignore", "-q", path])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn git_adds_humanize(command_lower: &str, project_root: &Path) -> bool {
    let command_lower = command_lower.to_lowercase();

    for segment in split_shell_segments(&command_lower) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        if !regex_is_match(
            r"(^|[[:space:]])git[[:space:]]+([^[:space:]]+[[:space:]]+)*add([[:space:]]|$)",
            segment,
        ) {
            continue;
        }

        let add_args = Regex::new(r".*?[[:space:]]add(?:[[:space:]]+(.*)|$)")
            .ok()
            .and_then(|regex| regex.captures(segment))
            .and_then(|captures| captures.get(1).map(|value| value.as_str()))
            .unwrap_or("");

        let normalized_args = add_args.replace(['"', '\''], "");
        if regex_is_match(
            r"(^|[[:space:]]|/)\.humanize($|/|[[:space:]])",
            &normalized_args,
        ) {
            return true;
        }

        let tokens = add_args.split_whitespace().collect::<Vec<_>>();
        let has_force = tokens.iter().any(|token| {
            *token == "--force"
                || token.strip_prefix('-').is_some_and(|flags| {
                    !flags.is_empty()
                        && flags.chars().all(|c| c.is_ascii_alphabetic())
                        && flags.contains('f')
                })
        });
        let has_all = tokens.iter().any(|token| {
            *token == "--all"
                || token.strip_prefix('-').is_some_and(|flags| {
                    !flags.is_empty()
                        && flags.chars().all(|c| c.is_ascii_alphabetic())
                        && flags.contains('a')
                })
        });
        let has_broad_scope = tokens
            .iter()
            .any(|token| matches!(token.replace(['"', '\''], "").as_str(), "." | "*" | "./"));

        if has_force && (has_all || has_broad_scope) {
            return true;
        }

        if !project_root.join(".humanize").is_dir() {
            continue;
        }

        if has_all {
            return true;
        }

        if has_broad_scope && !path_is_gitignored(project_root, ".humanize") {
            return true;
        }
    }

    false
}

fn command_modifies_file(command_lower: &str, file_pattern: &str) -> bool {
    let target = format!(r#"{}(?:$|[[:space:]/"'])"#, file_pattern);
    let patterns = [
        format!(r">[[:space:]]*[^[:space:]]*{}", target),
        format!(r">>[[:space:]]*[^[:space:]]*{}", target),
        format!(r"tee[[:space:]]+(-a[[:space:]]+)?[^[:space:]]*{}", target),
        format!(r"sed[[:space:]]+-i[^|\n]*{}", target),
        format!(r"awk[[:space:]]+-i[[:space:]]+inplace[^|\n]*{}", target),
        format!(r"perl[[:space:]]+-[^[:space:]]*i[^|\n]*{}", target),
        format!(r"(mv|cp)[[:space:]][^\n]*{}", target),
        format!(r"rm[[:space:]]+(-[rfv]+[[:space:]]+)?[^\n]*{}", target),
        format!(r"dd[[:space:]].*of=[^[:space:]]*{}", target),
        format!(r"truncate[[:space:]][^|\n]*{}", target),
        format!(r"printf[[:space:]].*>[[:space:]]*[^[:space:]]*{}", target),
        format!(
            r"exec[[:space:]]+[0-9]*>[[:space:]]*[^[:space:]]*{}",
            target
        ),
        format!(r"python(?:3)?[[:space:]]+-c[^\n]*{}", target),
        format!(r"node[[:space:]]+-e[^\n]*{}", target),
        format!(r"ruby[[:space:]]+-i[^\n]*{}", target),
        format!(r"touch[[:space:]][^\n]*{}", target),
        format!(r"install[[:space:]][^\n]*{}", target),
        format!(r"ed[[:space:]][^\n]*{}", target),
        format!(r"ex[[:space:]][^\n]*{}", target),
    ];

    patterns
        .iter()
        .any(|pattern| regex_is_match(pattern, command_lower))
}

fn is_cancel_authorized(active_loop_dir: &Path, command_lower: &str) -> bool {
    if !active_loop_dir.join(".cancel-requested").is_file() {
        return false;
    }

    if command_lower.contains("$(")
        || command_lower.contains('`')
        || command_lower.contains('\n')
        || command_lower.contains(';')
        || command_lower.contains("&&")
        || command_lower.contains("||")
        || command_lower.contains("|&")
        || command_lower.contains('|')
    {
        return false;
    }

    let trailing_whitespace = command_lower
        .chars()
        .rev()
        .take_while(|ch| ch.is_whitespace())
        .count();
    if trailing_whitespace >= 2 {
        return false;
    }

    let loop_dir_lower = normalize_shell_path(&format!("{}/", active_loop_dir.display()));
    let normalized = command_lower
        .replace("${loop_dir}", &loop_dir_lower)
        .replace("$loop_dir", &loop_dir_lower);

    if normalized.contains('$') {
        return false;
    }

    let Some(rest) = normalized.strip_prefix("mv") else {
        return false;
    };
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }

    let rest = rest.trim_start();
    let Some((src, rest)) = parse_shell_arg(rest) else {
        return false;
    };
    let Some((dest, rest)) = parse_shell_arg(rest) else {
        return false;
    };
    if !rest.trim().is_empty() {
        return false;
    }

    if matches!(
        (src.quote, dest.quote),
        (QuoteStyle::Single, QuoteStyle::Double) | (QuoteStyle::Double, QuoteStyle::Single)
    ) {
        return false;
    }

    let src = normalize_shell_path(src.value);
    let dest = normalize_shell_path(dest.value);
    let expected_src_state = format!("{}state.md", loop_dir_lower);
    let expected_src_finalize = format!("{}finalize-state.md", loop_dir_lower);
    let expected_dest = format!("{}cancel-state.md", loop_dir_lower);

    if src != expected_src_state && src != expected_src_finalize {
        return false;
    }

    if dest != expected_dest {
        return false;
    }

    let source_file = if src == expected_src_finalize {
        active_loop_dir.join("finalize-state.md")
    } else {
        active_loop_dir.join("state.md")
    };

    std::fs::symlink_metadata(source_file)
        .map(|metadata| !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn git_add_humanize_blocked_reason() -> String {
    render_template_or_fallback(
        "block/git-add-humanize.md",
        "# Git Add Blocked: .humanize Protection\n\nAdding .humanize files to version control is not allowed during an active loop.",
        &[],
    )
}

fn summary_bash_blocked_reason(correct_path: &str) -> String {
    render_template_or_fallback(
        "block/summary-bash-write.md",
        "# Bash Write Blocked: Use Write or Edit Tool\n\nDo not use Bash commands to modify summary files.\n\nUse the Write or Edit tool instead: {{CORRECT_PATH}}",
        &[("CORRECT_PATH", correct_path.to_string())],
    )
}

fn goal_tracker_bash_blocked_reason(correct_path: &str) -> String {
    render_template_or_fallback(
        "block/goal-tracker-bash-write.md",
        "# Bash Write Blocked: Use Write or Edit Tool\n\nDo not use Bash commands to modify goal-tracker.md.\n\nUse the Write or Edit tool instead: {{CORRECT_PATH}}",
        &[("CORRECT_PATH", correct_path.to_string())],
    )
}

fn goal_tracker_blocked_reason(current_round: u32, summary_file: &str) -> String {
    render_template_or_fallback(
        "block/goal-tracker-modification.md",
        "# Goal Tracker Modification Blocked (Round {{CURRENT_ROUND}})\n\nAfter Round 0, only Codex can modify the Goal Tracker. Include a Goal Tracker Update Request in your summary file: {{SUMMARY_FILE}}",
        &[
            ("CURRENT_ROUND", current_round.to_string()),
            ("SUMMARY_FILE", summary_file.to_string()),
        ],
    )
}

fn todos_bash_blocked_reason() -> String {
    render_template_or_fallback(
        "block/todos-file-access.md",
        "# Todos File Access Blocked\n\nDo NOT create or access round-*-todos.md files. Use the native Task tools instead.",
        &[],
    )
}

fn plan_backup_protected_reason() -> String {
    render_template_or_fallback(
        "block/plan-backup-protected.md",
        "Writing to plan.md backup is not allowed during RLCR loop.",
        &[],
    )
}

fn protected_state_reason(path_lower: &str) -> String {
    if path_lower.contains(".humanize/pr-loop/") {
        return render_template_or_fallback(
            "block/pr-loop-state-modification.md",
            "# PR Loop State File Modification Blocked\n\nYou cannot modify state.md in .humanize/pr-loop/.",
            &[],
        );
    }

    if path_lower.ends_with("finalize-state.md") {
        return render_template_or_fallback(
            "block/finalize-state-file-modification.md",
            "# Finalize State File Modification Blocked\n\nYou cannot modify finalize-state.md.",
            &[],
        );
    }

    render_template_or_fallback(
        "block/state-file-modification.md",
        "# State File Modification Blocked\n\nYou cannot modify state.md.",
        &[],
    )
}

fn protected_expansion_reason(cmd_trimmed: &str) -> String {
    format!(
        "Command uses shell expansion while targeting protected loop files, which is not allowed during an active loop: {}",
        cmd_trimmed
    )
}

fn find_active_pr_loop(loop_base_dir: &Path) -> Option<PathBuf> {
    let newest_dir = newest_session_dir(loop_base_dir)?;
    newest_dir.join("state.md").is_file().then_some(newest_dir)
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
/// Implements parity with loop-bash-validator.sh for protected RLCR/PR loop files.
pub(super) fn validate_bash(input: &HookInput) -> HookOutput {
    let command = match get_command(input) {
        Some(c) => c,
        None => return HookOutput::allow(),
    };

    let cmd_lower = command.to_lowercase();
    let cmd_trimmed = command.trim();

    if is_safe_bash_command(&cmd_lower) {
        return HookOutput::allow();
    }

    let project_root = std::env::var("CLAUDE_PROJECT_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let Some(project_root) = project_root else {
        return HookOutput::allow();
    };

    let active_loop_dir = humanize_core::state::find_active_loop(
        &project_root.join(".humanize/rlcr"),
        input.session_id.as_deref(),
    );
    let active_pr_loop_dir = find_active_pr_loop(&project_root.join(".humanize/pr-loop"));

    if active_loop_dir.is_none() && active_pr_loop_dir.is_none() {
        return HookOutput::allow();
    }

    let current_round = if let Some(loop_dir) = &active_loop_dir {
        let Some(state_file) = humanize_core::state::resolve_active_state_file(loop_dir) else {
            return HookOutput::allow();
        };
        let Ok(state_content) = std::fs::read_to_string(&state_file) else {
            return HookOutput::allow();
        };
        let Ok(state) = humanize_core::state::State::from_markdown_strict(&state_content) else {
            return HookOutput::block("Malformed state file, blocking operation for safety");
        };
        Some(state.current_round)
    } else {
        None
    };

    if git_adds_humanize(&cmd_lower, &project_root) {
        return HookOutput::block(git_add_humanize_blocked_reason());
    }

    if has_safe_command_blockers(&cmd_lower) && contains_protected_pattern(&cmd_lower).is_some() {
        if let Some(loop_dir) = &active_loop_dir {
            if (cmd_lower.contains("state.md") || cmd_lower.contains("finalize-state.md"))
                && is_cancel_authorized(loop_dir, &cmd_lower)
            {
                return HookOutput::allow();
            }
        }

        if cmd_lower.contains("${") || regex_is_match(r"\$[a-z_][a-z0-9_]*", &cmd_lower) {
            return HookOutput::block(protected_expansion_reason(cmd_trimmed));
        }
    }

    if let Some(loop_dir) = &active_loop_dir {
        if command_modifies_file(&cmd_lower, "finalize-state\\.md") {
            if is_cancel_authorized(loop_dir, &cmd_lower) {
                return HookOutput::allow();
            }
            return HookOutput::block(protected_state_reason("finalize-state.md"));
        }

        if command_modifies_file(&cmd_lower, "state\\.md") {
            if is_cancel_authorized(loop_dir, &cmd_lower) {
                return HookOutput::allow();
            }
            return HookOutput::block(protected_state_reason("state.md"));
        }

        if command_modifies_file(&cmd_lower, "\\.humanize/rlcr(/[^/]+)?/plan\\.md") {
            return HookOutput::block(plan_backup_protected_reason());
        }

        if command_modifies_file(&cmd_lower, "goal-tracker\\.md") {
            let round = current_round.unwrap_or(0);
            if round == 0 {
                let correct_path = loop_dir.join("goal-tracker.md");
                return HookOutput::block(goal_tracker_bash_blocked_reason(
                    &correct_path.display().to_string(),
                ));
            }

            let summary_file = loop_dir.join(format!("round-{}-summary.md", round));
            return HookOutput::block(goal_tracker_blocked_reason(
                round,
                &summary_file.display().to_string(),
            ));
        }

        if command_modifies_file(&cmd_lower, "round-[0-9]+-prompt\\.md") {
            return HookOutput::block(render_template_or_fallback(
                "block/prompt-file-write.md",
                "# Prompt File Write Blocked\n\nYou cannot write to round-*-prompt.md files.",
                &[],
            ));
        }

        if command_modifies_file(&cmd_lower, "round-[0-9]+-summary\\.md") {
            let summary_path =
                loop_dir.join(format!("round-{}-summary.md", current_round.unwrap_or(0)));
            return HookOutput::block(summary_bash_blocked_reason(
                &summary_path.display().to_string(),
            ));
        }

        if command_modifies_file(&cmd_lower, "round-[0-9]+-todos\\.md") {
            return HookOutput::block(todos_bash_blocked_reason());
        }
    }

    if active_pr_loop_dir.is_some() {
        if command_modifies_file(&cmd_lower, "\\.humanize/pr-loop(/[^/]+)?/state\\.md")
            || command_modifies_file(&cmd_lower, "state\\.md")
        {
            return HookOutput::block(protected_state_reason(".humanize/pr-loop/state.md"));
        }

        for pattern in PR_LOOP_READONLY_PATTERNS {
            if command_modifies_file(
                &cmd_lower,
                &format!("\\.humanize/pr-loop(/[^/]+)?/{pattern}"),
            ) || command_modifies_file(&cmd_lower, pattern)
            {
                return HookOutput::block(pr_loop_readonly_reason());
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn run(cmd: &mut Command) {
        let status = cmd.status().unwrap();
        assert!(status.success());
    }

    fn init_git_repo() -> TempDir {
        let tempdir = tempfile::tempdir().unwrap();
        run(Command::new("git")
            .args(["init", "-q"])
            .current_dir(tempdir.path()));
        run(Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(tempdir.path()));
        run(Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tempdir.path()));
        tempdir
    }

    #[test]
    fn command_modifies_file_detects_dangerous_variants() {
        assert!(command_modifies_file("rm state.md", "state\\.md"));
        assert!(command_modifies_file("mv state.md /tmp/foo", "state\\.md"));
        assert!(command_modifies_file("echo bad > state.md", "state\\.md"));
        assert!(command_modifies_file(
            "sed -i 's/x/y/' state.md",
            "state\\.md"
        ));
        assert!(command_modifies_file("sh -c 'rm state.md'", "state\\.md"));
        assert!(command_modifies_file("exec 3>state.md", "state\\.md"));

        assert!(!command_modifies_file("cat state.md", "state\\.md"));
        assert!(!command_modifies_file("git status", "state\\.md"));
        assert!(!command_modifies_file("ls -la", "state\\.md"));
        assert!(!command_modifies_file("grep pattern file", "state\\.md"));
    }

    #[test]
    fn git_adds_humanize_detects_direct_and_broad_patterns() {
        let repo = init_git_repo();
        std::fs::create_dir_all(repo.path().join(".humanize/rlcr")).unwrap();

        assert!(git_adds_humanize(r#"git add ".humanize""#, repo.path()));
        assert!(git_adds_humanize("git add -A", repo.path()));
        assert!(git_adds_humanize("git add -f .", repo.path()));
        assert!(git_adds_humanize("git -C subdir add --all", repo.path()));

        std::fs::write(repo.path().join(".gitignore"), ".humanize/\n").unwrap();
        run(Command::new("git")
            .args(["add", ".gitignore"])
            .current_dir(repo.path()));
        run(Command::new("git")
            .args(["commit", "-q", "-m", "ignore humanize"])
            .current_dir(repo.path()));

        assert!(!git_adds_humanize("git add .", repo.path()));
    }

    #[test]
    fn cancel_authorized_with_signal_and_exact_mv() {
        let tempdir = tempfile::tempdir().unwrap();
        let loop_dir = tempdir.path().join("2026-03-20_00-00-00");
        std::fs::create_dir_all(&loop_dir).unwrap();
        std::fs::write(loop_dir.join(".cancel-requested"), "").unwrap();
        std::fs::write(loop_dir.join("state.md"), "---\n---\n").unwrap();

        assert!(is_cancel_authorized(
            &loop_dir,
            &format!(
                "mv {}/state.md {}/cancel-state.md",
                loop_dir.display(),
                loop_dir.display()
            )
            .to_lowercase()
        ));
    }

    #[test]
    fn cancel_unauthorized_without_signal_or_with_injection() {
        let tempdir = tempfile::tempdir().unwrap();
        let loop_dir = tempdir.path().join("2026-03-20_00-00-00");
        std::fs::create_dir_all(&loop_dir).unwrap();
        std::fs::write(loop_dir.join("state.md"), "---\n---\n").unwrap();

        let valid_mv = format!(
            "mv {}/state.md {}/cancel-state.md",
            loop_dir.display(),
            loop_dir.display()
        )
        .to_lowercase();
        assert!(!is_cancel_authorized(&loop_dir, &valid_mv));

        std::fs::write(loop_dir.join(".cancel-requested"), "").unwrap();
        assert!(!is_cancel_authorized(
            &loop_dir,
            &format!("{valid_mv}; echo pwned")
        ));
        assert!(!is_cancel_authorized(
            &loop_dir,
            &format!("mv $(pwd)/state.md {}/cancel-state.md", loop_dir.display()).to_lowercase()
        ));
        assert!(!is_cancel_authorized(
            &loop_dir,
            &format!(
                "mv '{}/state.md' \"{}/cancel-state.md\"",
                loop_dir.display(),
                loop_dir.display()
            )
            .to_lowercase()
        ));
    }
}
