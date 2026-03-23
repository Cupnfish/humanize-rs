use super::pr::*;
use super::*;

pub(super) fn handle_cancel(cmd: CancelCommands) -> Result<()> {
    match cmd {
        CancelCommands::Rlcr { force } => cancel_rlcr_native(force),
        CancelCommands::Pr { force } => cancel_pr_native(force),
    }
}

pub(super) fn handle_resume(cmd: ResumeCommands) -> Result<()> {
    match cmd {
        ResumeCommands::Rlcr => resume_rlcr_native(),
        ResumeCommands::Pr => resume_pr_native(),
    }
}

fn cancel_rlcr_native(force: bool) -> Result<()> {
    let project_root = resolve_project_root()?;
    let loop_base_dir = project_root.join(".humanize/rlcr");

    let Some(loop_dir) = humanize_core::state::find_active_loop(&loop_base_dir, None) else {
        println!("NO_LOOP");
        println!("No active RLCR loop found.");
        std::process::exit(1);
    };

    let state_file = loop_dir.join("state.md");
    let finalize_state_file = loop_dir.join("finalize-state.md");
    let active_state = if state_file.exists() {
        state_file
    } else if finalize_state_file.exists() {
        finalize_state_file
    } else {
        println!("NO_ACTIVE_LOOP");
        println!(
            "No active RLCR loop found. The loop directory exists but no active state file is present."
        );
        std::process::exit(1);
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
        println!(
            "After this phase completes, the loop will end without returning to Codex review."
        );
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

    if active_state.ends_with("finalize-state.md") {
        println!("CANCELLED_FINALIZE");
        println!(
            "Cancelled RLCR loop during Finalize Phase (was at round {} of {}).",
            state.current_round, state.max_iterations
        );
        println!("State preserved as cancel-state.md");
    } else {
        println!("CANCELLED");
        println!(
            "Cancelled RLCR loop (was at round {} of {}).",
            state.current_round, state.max_iterations
        );
        println!("State preserved as cancel-state.md");
    }
    Ok(())
}

fn cancel_pr_native(_force: bool) -> Result<()> {
    let project_root = resolve_project_root()?;
    let loop_base_dir = project_root.join(".humanize/pr-loop");

    let Some(loop_dir) = newest_active_pr_loop(&loop_base_dir) else {
        println!("NO_LOOP");
        println!("No active PR loop found.");
        std::process::exit(1);
    };

    let state_file = loop_dir.join("state.md");
    if !state_file.exists() {
        println!("NO_ACTIVE_LOOP");
        println!(
            "No active PR loop found. The loop directory exists but no active state file is present."
        );
        std::process::exit(1);
    }
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

fn resume_rlcr_native() -> Result<()> {
    let project_root = resolve_project_root()?;
    let loop_base_dir = project_root.join(".humanize/rlcr");
    let Some(loop_dir) = humanize_core::state::find_active_loop(&loop_base_dir, None) else {
        println!("NO_LOOP");
        println!("No active RLCR loop found.");
        std::process::exit(1);
    };

    let Some(state_file) = humanize_core::state::resolve_active_state_file(&loop_dir) else {
        println!("NO_ACTIVE_LOOP");
        println!("No active RLCR state file found.");
        std::process::exit(1);
    };
    let state_content = match fs::read_to_string(&state_file) {
        Ok(content) => content,
        Err(_) => {
            println!("MALFORMED_STATE");
            println!("Malformed RLCR state file, cannot resume safely");
            std::process::exit(3);
        }
    };
    let mut state = match humanize_core::state::State::from_markdown_strict(&state_content) {
        Ok(state) => state,
        Err(_) => {
            return resume_rlcr_legacy_recovery(
                &project_root,
                &loop_dir,
                &state_file,
                &state_content,
            );
        }
    };

    arm_resume_session_handshake(
        &project_root,
        &state_file,
        &mut state,
        "humanize resume rlcr",
    )?;

    let (phase, action_path, action_content) =
        rlcr_resume_action(&project_root, &loop_dir, &state_file, &state)?;

    println!("=== resume-rlcr-loop ===\n");
    println!("Loop Directory: {}", loop_dir.display());
    println!("State File: {}", state_file.display());
    println!("Status: {}", state_status_label(&state_file));
    println!("Phase: {}", phase);
    println!("Round: {} / {}", state.current_round, state.max_iterations);
    println!(
        "Plan File: {}",
        if state.plan_file.is_empty() {
            "N/A".to_string()
        } else {
            state.plan_file.clone()
        }
    );
    println!(
        "Start Branch: {}",
        if state.start_branch.is_empty() {
            "N/A".to_string()
        } else {
            state.start_branch.clone()
        }
    );
    println!(
        "Base Branch: {}",
        if state.base_branch.is_empty() {
            "N/A".to_string()
        } else {
            state.base_branch.clone()
        }
    );
    println!("Action File: {}", action_path);
    println!("Session Rebind: armed");
    println!();
    print!("{}", action_content);
    if !action_content.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn resume_rlcr_legacy_recovery(
    project_root: &Path,
    loop_dir: &Path,
    state_file: &Path,
    state_content: &str,
) -> Result<()> {
    let state = humanize_core::state::State::from_markdown(state_content).unwrap_or_else(|_| {
        println!("MALFORMED_STATE");
        println!("Malformed RLCR state file, cannot resume safely");
        std::process::exit(3);
    });

    let (phase, action_path, action_content) =
        rlcr_resume_action(project_root, loop_dir, state_file, &state)?;

    println!("=== resume-rlcr-loop ===\n");
    println!("Loop Directory: {}", loop_dir.display());
    println!("State File: {}", state_file.display());
    println!("Status: {}", state_status_label(&state_file));
    println!("Phase: {}", phase);
    println!("Round: {} / {}", state.current_round, state.max_iterations);
    println!("State Schema: legacy");
    println!("Session Rebind: skipped");
    println!();
    println!(
        "This RLCR loop was started by an older Humanize version. The loop data is still intact, but the current CLI cannot safely rebind the host session to this state."
    );
    println!(
        "Use the artifacts below to recover unfinished work, then start a new RLCR loop if you need current-version automation."
    );
    println!("Action File: {}", action_path);
    println!();
    print!("{}", action_content);
    if !action_content.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn resume_pr_native() -> Result<()> {
    let project_root = resolve_project_root()?;
    let loop_base_dir = project_root.join(".humanize/pr-loop");
    let Some(loop_dir) = newest_active_pr_loop(&loop_base_dir) else {
        println!("NO_LOOP");
        println!("No active PR loop found.");
        std::process::exit(1);
    };

    let state_file = loop_dir.join("state.md");
    if !state_file.exists() {
        println!("NO_ACTIVE_LOOP");
        println!("No active PR loop state file found.");
        std::process::exit(1);
    }
    let state_content = match fs::read_to_string(&state_file) {
        Ok(content) => content,
        Err(_) => {
            println!("MALFORMED_STATE");
            println!("Malformed PR loop state file, cannot resume safely");
            std::process::exit(3);
        }
    };
    let state = humanize_core::state::State::from_markdown(&state_content).unwrap_or_else(|_| {
        println!("MALFORMED_STATE");
        println!("Malformed PR loop state file, cannot resume safely");
        std::process::exit(3);
    });

    let (phase, action_path, action_content) = pr_resume_action(&loop_dir, &state);

    println!("=== resume-pr-loop ===\n");
    println!("Loop Directory: {}", loop_dir.display());
    println!("State File: {}", state_file.display());
    println!("Status: {}", state_status_label(&state_file));
    println!("Phase: {}", phase);
    println!("Round: {} / {}", state.current_round, state.max_iterations);
    println!(
        "PR Number: {}",
        state
            .pr_number
            .map(|value| format!("#{value}"))
            .unwrap_or_else(|| "N/A".to_string())
    );
    println!(
        "Configured Bots: {}",
        join_list(&state.configured_bots.clone().unwrap_or_default(), "none")
    );
    println!(
        "Active Bots: {}",
        join_list(&state.active_bots.clone().unwrap_or_default(), "none")
    );
    println!("Action File: {}", action_path);
    println!();
    print!("{}", action_content);
    if !action_content.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn arm_resume_session_handshake(
    project_root: &Path,
    state_file: &Path,
    state: &mut humanize_core::state::State,
    command_signature: &str,
) -> Result<()> {
    state.session_id = None;
    state.save(state_file)?;
    fs::create_dir_all(project_root.join(".humanize"))?;
    fs::write(
        project_root.join(".humanize/.pending-session-id"),
        format!("{}\n{}\n", state_file.display(), command_signature),
    )?;
    Ok(())
}

fn rlcr_resume_action(
    project_root: &Path,
    loop_dir: &Path,
    state_file: &Path,
    state: &humanize_core::state::State,
) -> Result<(String, String, String)> {
    if state_file.ends_with("finalize-state.md") {
        let finalize_summary_file = loop_dir.join("finalize-summary.md");
        if !finalize_summary_file.exists() {
            fs::write(
                &finalize_summary_file,
                "# Finalize Summary\n\nDocument the final simplification pass here.\n",
            )?;
        }
        return Ok((
            "finalize".to_string(),
            finalize_summary_file.display().to_string(),
            build_finalize_phase_prompt(state, loop_dir, &finalize_summary_file),
        ));
    }

    if state.review_started {
        let prompt_file = loop_dir.join(format!("round-{}-prompt.md", state.current_round));
        let review_result_file =
            loop_dir.join(format!("round-{}-review-result.md", state.current_round));
        let skip_impl_marker = loop_dir.join(".review-phase-started");
        let skip_impl_ready = skip_impl_marker.exists()
            && prompt_file.exists()
            && !review_result_file.exists()
            && !loop_dir
                .join(format!(
                    "round-{}-review-prompt.md",
                    state.current_round + 1
                ))
                .exists();

        if review_result_file.exists() && prompt_file.exists() {
            return Ok((
                "review-fix".to_string(),
                prompt_file.display().to_string(),
                fs::read_to_string(&prompt_file)?,
            ));
        }

        if skip_impl_ready {
            return Ok((
                "review-ready".to_string(),
                prompt_file.display().to_string(),
                fs::read_to_string(&prompt_file)?,
            ));
        }

        let review_round = state.current_round + 1;
        let review_prompt = loop_dir.join(format!("round-{}-review-prompt.md", review_round));
        let review_log = rlcr_cache_dir(project_root, loop_dir)
            .ok()
            .map(|cache_dir| cache_dir.join(format!("round-{}-codex-review.log", review_round)));
        let action_path = if review_prompt.exists() {
            review_prompt.display().to_string()
        } else {
            state_file.display().to_string()
        };
        let mut content = String::from(
            "# Resume Review Phase\n\nThe RLCR loop is in review phase and no local fix prompt is currently pending.\n\nContinue working in the host, then stop again so Humanize can retry the Codex review.\n",
        );
        if review_prompt.exists() {
            content.push_str(&format!("\nReview Prompt: {}\n", review_prompt.display()));
        }
        if let Some(log_path) = review_log {
            content.push_str(&format!("Review Log: {}\n", log_path.display()));
        }
        return Ok(("review-pending".to_string(), action_path, content));
    }

    let prompt_file = loop_dir.join(format!("round-{}-prompt.md", state.current_round));
    if prompt_file.exists() {
        return Ok((
            "implementation".to_string(),
            prompt_file.display().to_string(),
            fs::read_to_string(&prompt_file)?,
        ));
    }

    if state.current_round > 0 {
        let previous_round = state.current_round - 1;
        let previous_review_result =
            loop_dir.join(format!("round-{}-review-result.md", previous_round));
        let summary_file = loop_dir.join(format!("round-{}-summary.md", state.current_round));
        if previous_review_result.exists() {
            return Ok((
                "implementation".to_string(),
                previous_review_result.display().to_string(),
                format!(
                    "# Resume Implementation\n\nThe loop is in implementation phase for round {}.\n\nThe next round prompt file is missing, but the previous Codex review result is still available.\n\n- Review Result: {}\n- Write Summary To: {}\n- Loop Directory: {}\n",
                    state.current_round,
                    previous_review_result.display(),
                    summary_file.display(),
                    loop_dir.display()
                ),
            ));
        }
    }

    let fallback_path = loop_dir.join("goal-tracker.md");
    Ok((
        "implementation".to_string(),
        fallback_path.display().to_string(),
        format!(
            "# Resume RLCR\n\nNo round prompt file was found.\n\nInspect the loop directory and continue from the latest artifacts.\n\n- Goal Tracker: {}\n- Loop Directory: {}\n",
            fallback_path.display(),
            loop_dir.display()
        ),
    ))
}

fn pr_resume_action(
    loop_dir: &Path,
    state: &humanize_core::state::State,
) -> (String, String, String) {
    let round_feedback = loop_dir.join(format!("round-{}-pr-feedback.md", state.current_round));
    if round_feedback.exists() {
        return (
            "bot-feedback".to_string(),
            round_feedback.display().to_string(),
            read_monitor_content(&round_feedback),
        );
    }

    let round_prompt = loop_dir.join("round-0-prompt.md");
    if state.current_round == 0 && round_prompt.exists() {
        return (
            "initial".to_string(),
            round_prompt.display().to_string(),
            read_monitor_content(&round_prompt),
        );
    }

    let resolve_file = loop_dir.join(format!("round-{}-pr-resolve.md", state.current_round));
    let comment_file = loop_dir.join(format!("round-{}-pr-comment.md", state.current_round + 1));
    (
        "active".to_string(),
        resolve_file.display().to_string(),
        format!(
            "# Resume PR Loop\n\nContinue from the existing PR loop state.\n\n- Resolution Summary: {}\n- Latest Bot Comments: {}\n- Loop Directory: {}\n",
            resolve_file.display(),
            comment_file.display(),
            loop_dir.display()
        ),
    )
}

fn read_monitor_content(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|_| format!("Unable to read {}", path.display()))
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
