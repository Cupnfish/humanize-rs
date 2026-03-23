use super::pr::*;
use super::*;
use humanize_core::state::PlanMode;
use sha2::{Digest, Sha256};

#[derive(Debug, Default, Deserialize)]
struct StopHookInput {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    transcript_path: Option<String>,
    #[serde(default)]
    stop_hook_active: bool,
    #[serde(default)]
    last_assistant_message: Option<String>,
}

#[derive(Debug, Serialize)]
struct StopHookOutput {
    decision: String,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "systemMessage")]
    system_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StopFailureMarker {
    session_id: String,
    error: String,
    recorded_at_epoch: i64,
    #[serde(default)]
    last_assistant_message: Option<String>,
}

const STOP_FAILURE_MARKER_MAX_AGE_SECS: i64 = 15 * 60;

pub(super) fn handle_stop(cmd: StopCommands) -> Result<()> {
    match cmd {
        StopCommands::Rlcr => handle_stop_rlcr(),
        StopCommands::Pr => handle_stop_pr(),
    }
}

pub(super) fn handle_stop_failure_hook(input: &HookInput) -> Result<()> {
    let Some(session_id) = input
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let Some(error) = input.error.as_deref() else {
        return Ok(());
    };
    if !is_stop_failure_bypass_error(error) {
        return Ok(());
    }

    let project_root = resolve_project_root()?;
    let marker = StopFailureMarker {
        session_id: session_id.to_string(),
        error: error.to_string(),
        recorded_at_epoch: current_unix_epoch(),
        last_assistant_message: input.last_assistant_message.clone(),
    };

    let path = stop_failure_marker_path(&project_root, session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string(&marker)?)?;
    Ok(())
}

fn is_stop_failure_bypass_error(error: &str) -> bool {
    matches!(
        error.trim(),
        "rate_limit" | "billing_error" | "authentication_failed"
    )
}

fn stop_failure_marker_path(project_root: &Path, session_id: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    project_root
        .join(".humanize/stop-failure-markers")
        .join(format!("{digest}.json"))
}

fn consume_stop_failure_bypass(project_root: &Path, input: &StopHookInput) -> bool {
    if !input.stop_hook_active {
        return false;
    }

    let Some(session_id) = input
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return false;
    };

    let path = stop_failure_marker_path(project_root, session_id);
    let raw = match fs::read_to_string(&path) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let marker: StopFailureMarker = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => {
            let _ = fs::remove_file(&path);
            return false;
        }
    };

    let age = current_unix_epoch().saturating_sub(marker.recorded_at_epoch);
    let message_matches = match (
        marker.last_assistant_message.as_deref(),
        input.last_assistant_message.as_deref(),
    ) {
        (Some(expected), Some(actual)) => expected == actual,
        _ => true,
    };

    if marker.session_id != session_id || age > STOP_FAILURE_MARKER_MAX_AGE_SECS || !message_matches
    {
        if age > STOP_FAILURE_MARKER_MAX_AGE_SECS || !message_matches {
            let _ = fs::remove_file(&path);
        }
        return false;
    }

    let _ = fs::remove_file(&path);
    true
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

    if consume_stop_failure_bypass(&project_root, &input) {
        return Ok(());
    }

    let is_finalize_phase = state_file.ends_with("finalize-state.md");
    let state_content = fs::read_to_string(&state_file)
        .context("Malformed state file, blocking operation for safety")?;
    let mut state = humanize_core::state::State::from_markdown_strict(&state_content)
        .map_err(|_| anyhow::anyhow!("Malformed state file, blocking operation for safety"))?;

    if state.base_branch.is_empty() {
        let reason = "State file missing base_branch value. This indicates the loop was started with an older version of humanize.\n\n\
                     Options:\n1. Cancel the loop: /humanize:cancel-rlcr-loop\n2. Update humanize and restart the loop";
        return emit_stop_block(
            reason,
            Some("Loop: Blocked - state schema outdated (missing base_branch)"),
        );
    }

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
        return emit_stop_block(
            &reason,
            Some("Loop: Blocked - goal tracker placeholders remain"),
        );
    }

    if let Some(reason) = transcript_todo_block_reason(input.transcript_path.as_deref())? {
        return emit_stop_block(&reason, Some("Loop: Blocked - incomplete tasks detected"));
    }

    if state.review_started {
        return run_review_phase(&project_root, &loop_dir, &state_file, &mut state);
    }

    run_impl_phase(
        &project_root,
        &loop_dir,
        &state_file,
        &mut state,
        &summary_file,
    )
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
            );
        }
    };

    let review_result_file =
        loop_dir.join(format!("round-{}-review-result.md", state.current_round));
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
    let _combined_output = match humanize_core::codex::run_review(
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
                &format!(
                    "The `codex review` command failed: {}. Please retry the exit.",
                    err
                ),
                Some("Loop: Blocked - codex review failed"),
            );
        }
    };

    if let Some(review_issues) = detect_review_issues(review_round, loop_dir, &cache_dir)? {
        state.current_round = review_round;
        state.save(state_file)?;
        let next_prompt_file = loop_dir.join(format!("round-{}-prompt.md", review_round));
        let next_summary_file = loop_dir.join(format!("round-{}-summary.md", review_round));
        let review_result_file = loop_dir.join(format!("round-{}-review-result.md", review_round));
        let review_fix_prompt =
            build_review_phase_fix_prompt(&review_issues, &review_result_file, &next_summary_file);
        fs::write(&next_prompt_file, &review_fix_prompt)?;
        return emit_stop_block(
            &review_fix_prompt,
            Some("Loop: Blocked - review issues found"),
        );
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
    emit_stop_block(&finalize_prompt, Some("Loop: Blocked - finalize phase"))
}

fn combine_review_output(stdout: &str, stderr: &str) -> String {
    match (stdout.trim(), stderr.trim()) {
        ("", "") => String::new(),
        ("", s) => format!("{}\n", s),
        (s, "") => format!("{}\n", s),
        (s1, s2) => format!("{}\n{}\n", s1, s2),
    }
}

fn detect_review_issues(round: u32, loop_dir: &Path, cache_dir: &Path) -> Result<Option<String>> {
    let log_file = cache_dir.join(format!("round-{}-codex-review.log", round));
    if !log_file.is_file() {
        anyhow::bail!("Codex review log file not found: {}", log_file.display());
    }

    let content = fs::read_to_string(&log_file)?;
    if content.trim().is_empty() {
        anyhow::bail!("Codex review log file is empty: {}", log_file.display());
    }

    let lines = content.lines().collect::<Vec<_>>();
    let scan_start = lines.len().saturating_sub(50);
    let relative_match = lines[scan_start..]
        .iter()
        .position(|line| severity_marker_near_start(line));

    let Some(relative_match) = relative_match else {
        return Ok(None);
    };

    let start_index = scan_start + relative_match;
    let extracted = lines[start_index..].join("\n");
    let result_file = loop_dir.join(format!("round-{}-review-result.md", round));
    fs::write(&result_file, format!("{extracted}\n"))?;

    Ok(Some(format!("## Codex Review Issues\n\n{}", extracted)))
}

fn severity_marker_near_start(line: &str) -> bool {
    let prefix = line.chars().take(10).collect::<String>();
    let bytes = prefix.as_bytes();
    bytes.windows(4).any(|window| {
        window[0] == b'[' && window[1] == b'P' && window[2].is_ascii_digit() && window[3] == b']'
    })
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

    if fs::symlink_metadata(&backup_plan)
        .ok()
        .is_some_and(|metadata| metadata.file_type().is_symlink())
    {
        return Some("Plan snapshot in loop directory cannot be a symbolic link.".to_string());
    }

    let source_path = if state.plan_source_path.trim().is_empty() {
        &state.plan_file
    } else {
        &state.plan_source_path
    };

    if matches!(
        state.plan_mode,
        PlanMode::SourceClean | PlanMode::SourceImmutable
    ) {
        let full_plan = project_root.join(source_path);
        if !full_plan.exists() {
            return Some(format!(
                "Project plan file has been deleted.\n\nOriginal: {}",
                source_path
            ));
        }

        if matches!(state.plan_mode, PlanMode::SourceImmutable) {
            let tracked = git_path_is_tracked(project_root, source_path).ok()?;
            if tracked != state.plan_source_tracked_at_start {
                return Some(format!(
                    "Plan file tracking changed during RLCR loop.\n\nFile: {}",
                    source_path
                ));
            }
        }

        let tracked = git_path_is_tracked(project_root, source_path).ok()?;
        if tracked {
            let plan_status = git_path_status_porcelain(project_root, source_path).ok()?;
            if !plan_status.trim().is_empty() {
                return Some(format!(
                    "Plan file has uncommitted modifications.\n\nFile: {}\nStatus: {}",
                    source_path,
                    plan_status.trim()
                ));
            }
        }

        if matches!(state.plan_mode, PlanMode::SourceImmutable)
            && !state.plan_source_sha256.is_empty()
        {
            let current_content = fs::read(&full_plan).ok()?;
            let current_hash = {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&current_content);
                format!("{:x}", hasher.finalize())
            };
            if current_hash != state.plan_source_sha256 {
                return Some(format!(
                    "The plan file `{}` has been modified since the RLCR loop started.",
                    source_path
                ));
            }
        }
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

fn utf8_prefix_at_most(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }

    let mut end = 0;
    for (idx, ch) in text.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    &text[..end]
}

fn utf8_suffix_at_most(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }

    let mut start = text.len();
    for (idx, _) in text.char_indices().rev() {
        if text.len() - idx > max_bytes {
            break;
        }
        start = idx;
    }
    &text[start..]
}

const COMPACT_REASON_SEPARATOR: &str = "\n\n...\n\n";

fn important_stop_hook_reason_lines(reason: &str, max_inline_bytes: usize) -> Option<String> {
    let mut lines = Vec::new();
    let mut push_line = |line: &str| {
        let trimmed = line.trim();
        if trimmed.is_empty() || lines.iter().any(|existing: &String| existing == trimmed) {
            return;
        }
        let mut candidate = lines.clone();
        candidate.push(trimmed.to_string());
        let preview = candidate.join("\n\n");
        if preview.len() <= max_inline_bytes {
            lines.push(trimmed.to_string());
        }
    };

    if let Some(first) = reason.lines().find(|line| !line.trim().is_empty()) {
        push_line(first);
    }

    if let Some(line) = reason.lines().find(|line| {
        let trimmed = line.trim().to_ascii_lowercase();
        trimmed.contains(".md")
            && (trimmed.contains("review")
                || trimmed.contains("check")
                || trimmed.contains("feedback")
                || trimmed.contains("result"))
    }) {
        push_line(line);
    }

    for needle in [
        ".md",
        "Read that file carefully",
        "Write your summary to:",
        "Write your resolution summary to:",
    ] {
        if let Some(line) = reason.lines().find(|line| line.trim().contains(needle)) {
            push_line(line);
        }
    }

    (lines.len() > 1).then(|| lines.join("\n\n"))
}

fn compact_stop_hook_reason(reason: &str, max_inline_bytes: usize) -> Option<String> {
    if reason.len() <= max_inline_bytes {
        return None;
    }

    if let Some(summary) = important_stop_hook_reason_lines(reason, max_inline_bytes) {
        return Some(summary);
    }

    let available = max_inline_bytes.saturating_sub(COMPACT_REASON_SEPARATOR.len());

    if available == 0 {
        return Some(utf8_prefix_at_most(reason, max_inline_bytes).to_string());
    }

    let prefix_budget = available / 2;
    let suffix_budget = available.saturating_sub(prefix_budget);
    let mut compacted = utf8_prefix_at_most(reason, prefix_budget)
        .trim_end()
        .to_string();
    compacted.push_str(COMPACT_REASON_SEPARATOR);
    compacted.push_str(utf8_suffix_at_most(reason, suffix_budget).trim_start());

    if compacted.len() > max_inline_bytes {
        compacted = utf8_prefix_at_most(&compacted, max_inline_bytes).to_string();
    }

    Some(compacted)
}

fn emit_stop_block(reason: &str, system_message: Option<&str>) -> Result<()> {
    let config = stop_hook_prompt_config();
    let reason = if config.compact_large_prompts {
        compact_stop_hook_reason(reason, config.max_inline_bytes)
            .unwrap_or_else(|| reason.to_string())
    } else {
        reason.to_string()
    };
    let output = StopHookOutput {
        decision: "block".to_string(),
        reason,
        system_message: system_message.map(|msg| msg.to_string()),
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn best_effort_startup_case_value(
    result: Result<StartupCaseInfo>,
    previous: Option<&str>,
) -> String {
    match result {
        Ok(startup) => startup.case_num.to_string(),
        Err(err) => {
            eprintln!("Warning: Failed to re-evaluate startup case: {err}");
            previous.unwrap_or("1").to_string()
        }
    }
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

    if consume_stop_failure_bypass(&project_root, &_input) {
        return Ok(());
    }

    ensure_command_exists("gh", "Error: PR loop requires GitHub CLI (gh)")?;

    let state_content = fs::read_to_string(&state_file)
        .context("Malformed PR loop state file, blocking operation for safety")?;
    let mut state = humanize_core::state::State::from_markdown(&state_content).map_err(|_| {
        anyhow::anyhow!("Malformed PR loop state file, blocking operation for safety")
    })?;

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
    let pr_state = gh_pr_state_in_repo(&project_root, &pr_repo, pr_number).unwrap_or_default();
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
        anyhow::anyhow!(
            "Git status operation failed. Please check your repository state and try again."
        )
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
        let current_branch =
            git_current_branch(&project_root).unwrap_or_else(|_| "main".to_string());
        let reason = render_template_or_fallback(
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
            && !humanize_core::git::is_ancestor(&project_root, &previous_sha, &current_head)
                .unwrap_or(false)
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

            let reason = render_template_or_fallback(
                "block/force-push-detected.md",
                "# Force Push Detected\n\nA force push (history rewrite) has been detected. Post a new @bot trigger comment: {{BOT_MENTION_STRING}}",
                &[
                    ("OLD_COMMIT", previous_sha),
                    ("NEW_COMMIT", current_head.clone()),
                    (
                        "BOT_MENTION_STRING",
                        build_bot_mention_string(&configured_bots),
                    ),
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
    let commit_info =
        gh_pr_commit_info_in_repo(&project_root, &pr_repo, pr_number).unwrap_or(PrCommitInfo {
            latest_commit_sha: state.latest_commit_sha.clone().unwrap_or_default(),
            latest_commit_at: state.latest_commit_at.clone().unwrap_or_default(),
        });
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
        match gh_detect_trigger_comment(
            &project_root,
            &pr_repo,
            pr_number,
            user,
            &configured_bots,
            state.latest_commit_at.as_deref(),
        ) {
            Ok(Some(trigger)) => {
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
            Ok(None) => {}
            Err(err) => {
                eprintln!("Warning: Failed to detect trigger comment: {err}");
            }
        }
    }

    let startup_case = state
        .startup_case
        .clone()
        .unwrap_or_else(|| "1".to_string());
    let require_trigger =
        pr_requires_trigger(state.current_round, &startup_case, new_commits_detected);

    if active_bots.iter().any(|bot| bot == "codex") {
        let reaction_after = state
            .last_trigger_at
            .as_deref()
            .or(state.started_at.as_deref());
        if !(require_trigger && state.last_trigger_at.is_none()) {
            if gh_find_codex_thumbsup(&project_root, &pr_repo, pr_number, reaction_after)?.is_some()
            {
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
        let reason = render_template_or_fallback(
            "block/no-trigger-comment.md",
            "# Missing Trigger Comment\n\nNo @bot mention found. Please run: gh pr comment {{PR_NUMBER}} --body \"{{BOT_MENTION_STRING}} please review\"",
            &[
                ("STARTUP_CASE", startup_case.clone()),
                ("STARTUP_CASE_DESC", startup_case_desc.to_string()),
                ("CURRENT_ROUND", state.current_round.to_string()),
                (
                    "BOT_MENTION_STRING",
                    build_bot_mention_string(&configured_bots),
                ),
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
                let reason = render_template_or_fallback(
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
    let startup_case_value = best_effort_startup_case_value(
        gh_startup_case(
            &project_root,
            &pr_repo,
            pr_number,
            &configured_bots,
            &refreshed_commit_info.latest_commit_at,
        ),
        state.startup_case.as_deref(),
    );

    state.current_round = next_round;
    state.active_bots = Some(new_active_bots.clone());
    state.latest_commit_sha = Some(current_head);
    state.latest_commit_at = Some(refreshed_commit_info.latest_commit_at);
    state.startup_case = Some(startup_case_value);
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
        &check_file,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_review_issues_scans_only_tail_and_extracts_from_first_marker() {
        let tempdir = tempfile::tempdir().unwrap();
        let loop_dir = tempdir.path().join("loop");
        let cache_dir = tempdir.path().join("cache");
        fs::create_dir_all(&loop_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();

        let mut lines = (0..55).map(|idx| format!("line {idx}")).collect::<Vec<_>>();
        lines[2] = "[P1] old marker outside scan window".to_string();
        lines.push("[P2] issue in tail".to_string());
        lines.push("follow-up detail".to_string());
        fs::write(cache_dir.join("round-3-codex-review.log"), lines.join("\n")).unwrap();

        let extracted = detect_review_issues(3, &loop_dir, &cache_dir)
            .unwrap()
            .unwrap();
        let saved = fs::read_to_string(loop_dir.join("round-3-review-result.md")).unwrap();

        assert!(extracted.contains("[P2] issue in tail"));
        assert!(saved.starts_with("[P2] issue in tail"));
        assert!(!saved.contains("old marker outside scan window"));
    }

    #[test]
    fn detect_review_issues_returns_none_without_tail_marker() {
        let tempdir = tempfile::tempdir().unwrap();
        let loop_dir = tempdir.path().join("loop");
        let cache_dir = tempdir.path().join("cache");
        fs::create_dir_all(&loop_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();

        fs::write(
            cache_dir.join("round-2-codex-review.log"),
            "review complete\nno priorities found\n",
        )
        .unwrap();

        assert!(
            detect_review_issues(2, &loop_dir, &cache_dir)
                .unwrap()
                .is_none()
        );
        assert!(!loop_dir.join("round-2-review-result.md").exists());
    }

    #[test]
    fn best_effort_startup_case_value_preserves_previous_on_error() {
        let value = best_effort_startup_case_value(Err(anyhow::anyhow!("boom")), Some("4"));
        assert_eq!(value, "4");

        let defaulted = best_effort_startup_case_value(Err(anyhow::anyhow!("boom")), None);
        assert_eq!(defaulted, "1");
    }

    #[test]
    fn compact_stop_hook_reason_keeps_small_payloads() {
        assert!(compact_stop_hook_reason("short", 128).is_none());
    }

    #[test]
    fn compact_stop_hook_reason_truncates_large_payloads_with_notice() {
        let reason = "A".repeat(512);
        let compacted = compact_stop_hook_reason(&reason, 220).unwrap();

        assert!(compacted.len() <= 220, "compacted={compacted}");
        assert!(compacted.contains("..."), "compacted={compacted}");
        assert_ne!(compacted, reason);
    }
}
