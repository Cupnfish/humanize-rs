use super::pr::*;
use super::*;
use humanize_core::state::PlanMode;

#[derive(Debug, Clone)]
struct SetupRlcrOptions {
    positional_plan_file: Option<String>,
    explicit_plan_file: Option<String>,
    track_plan_file: bool,
    plan_lock: crate::PlanLockArg,
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

pub(super) fn handle_setup(cmd: SetupCommands) -> Result<()> {
    match cmd {
        SetupCommands::Rlcr {
            plan_file,
            plan_file_explicit,
            track_plan_file,
            plan_lock,
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
            plan_lock,
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

fn setup_rlcr_native(options: SetupRlcrOptions) -> Result<()> {
    let project_root = resolve_project_root()?;

    let chosen_plan = match (&options.positional_plan_file, &options.explicit_plan_file) {
        (Some(_), Some(_)) => {
            bail!("Error: cannot specify both positional plan file and --plan-file")
        }
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

    let start_branch = humanize_core::git::get_current_branch(&project_root)
        .context("Error: failed to get current branch")?;

    let (codex_model, codex_effort) = parse_model_and_effort(
        &options.codex_model,
        humanize_core::constants::DEFAULT_CODEX_EFFORT,
    );

    let loop_base_dir = project_root.join(".humanize/rlcr");
    fs::create_dir_all(&loop_base_dir)?;
    let timestamp = loop_timestamp();
    let loop_dir = loop_base_dir.join(&timestamp);
    fs::create_dir_all(&loop_dir)?;

    let mut line_count = 0usize;
    let plan_mode = plan_lock_mode(&options.plan_lock, options.track_plan_file);
    let snapshot_plan_path = format!(".humanize/rlcr/{}/plan.md", timestamp);

    let (
        state_plan_file,
        plan_source_path,
        plan_source_exists_at_start,
        plan_source_tracked_at_start,
        plan_source_sha256,
        plan_source_git_oid,
    ) = if let Some(plan_file) = chosen_plan.clone() {
        let validation = validate_setup_plan_file(&project_root, &plan_file, &plan_mode)?;
        line_count = validation.line_count;
        create_plan_backup(&validation.full_path, &loop_dir.join("plan.md"))?;
        (
            plan_file.clone(),
            plan_file,
            true,
            validation.tracked,
            validation.source_sha256,
            validation.source_git_oid,
        )
    } else {
        let placeholder = loop_dir.join("plan.md");
        fs::write(&placeholder, skip_impl_plan_placeholder())?;
        (
            snapshot_plan_path.clone(),
            snapshot_plan_path.clone(),
            true,
            false,
            String::new(),
            None,
        )
    };

    let base_branch = detect_base_branch(&project_root, options.base_branch.as_deref())?;
    let base_commit = git_rev_parse(&project_root, &base_branch)?;

    let state = humanize_core::state::State::new_rlcr(
        state_plan_file.clone(),
        matches!(plan_mode, PlanMode::SourceImmutable),
        plan_mode,
        plan_source_path,
        plan_source_exists_at_start,
        plan_source_tracked_at_start,
        plan_source_sha256,
        snapshot_plan_path,
        plan_source_git_oid,
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
    fs::write(
        &pending_session,
        format!(
            "{}\n{}\n",
            loop_dir.join("state.md").display(),
            "humanize setup rlcr"
        ),
    )?;

    if options.skip_impl {
        fs::write(
            loop_dir.join(".review-phase-started"),
            "build_finish_round=0\n",
        )?;
    }

    let goal_tracker_path = loop_dir.join("goal-tracker.md");
    let summary_path = loop_dir.join("round-0-summary.md");
    let prompt_path = loop_dir.join("round-0-prompt.md");

    if options.skip_impl {
        fs::write(&goal_tracker_path, skip_impl_goal_tracker())?;
        fs::write(
            &prompt_path,
            skip_impl_round_prompt(&summary_path, &base_branch, &start_branch),
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
    if humanize_core::state::find_active_loop(&project_root.join(".humanize/rlcr"), None).is_some()
    {
        bail!("Error: An RLCR loop is already active");
    }
    if newest_active_pr_loop(&project_root.join(".humanize/pr-loop")).is_some() {
        bail!("Error: A PR loop is already active");
    }

    ensure_command_exists(
        "gh",
        "Error: start-pr-loop requires the GitHub CLI (gh) to be installed",
    )?;
    ensure_command_exists("codex", "Error: start-pr-loop requires codex to run")?;
    ensure_gh_auth(&project_root)?;

    let start_branch = humanize_core::git::get_current_branch(&project_root)
        .context("Error: Failed to get current branch")?;
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
    let startup = gh_startup_case(
        &project_root,
        &current_repo,
        pr_number,
        &active_bots,
        &commit_info.latest_commit_at,
    )?;

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
            .args([
                "pr",
                "comment",
                &pr_number.to_string(),
                "--repo",
                &current_repo,
                "--body",
                &body,
            ])
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

    let (codex_model, codex_effort) = parse_model_and_effort(&options.codex_model, "medium");

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

fn parse_model_and_effort(spec: &str, default_effort: &str) -> (String, String) {
    match spec.split_once(':') {
        Some((model, effort)) => (model.to_string(), effort.to_string()),
        None => (spec.to_string(), default_effort.to_string()),
    }
}

struct ValidatedPlan {
    full_path: PathBuf,
    line_count: usize,
    tracked: bool,
    source_sha256: String,
    source_git_oid: Option<String>,
}

fn plan_lock_mode(arg: &crate::PlanLockArg, track_plan_file: bool) -> PlanMode {
    if track_plan_file {
        PlanMode::SourceImmutable
    } else {
        match arg {
            crate::PlanLockArg::Snapshot => PlanMode::Snapshot,
            crate::PlanLockArg::SourceClean => PlanMode::SourceClean,
            crate::PlanLockArg::SourceImmutable => PlanMode::SourceImmutable,
        }
    }
}

fn validate_setup_plan_file(
    project_root: &Path,
    plan_file: &str,
    plan_mode: &PlanMode,
) -> Result<ValidatedPlan> {
    if Path::new(plan_file).is_absolute() {
        bail!(
            "Error: Plan file must be a relative path, got: {}",
            plan_file
        );
    }
    if plan_file.chars().any(char::is_whitespace) {
        bail!("Error: Plan file path cannot contain spaces");
    }
    if plan_file.chars().any(|c| {
        matches!(
            c,
            ';' | '&'
                | '|'
                | '$'
                | '`'
                | '<'
                | '>'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '!'
                | '#'
                | '~'
                | '*'
                | '?'
                | '\\'
        )
    }) {
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
    match plan_mode {
        PlanMode::Snapshot => {}
        PlanMode::SourceClean => {
            if tracked && !plan_status.trim().is_empty() {
                bail!("Error: --plan-lock source-clean requires tracked plan file to be clean (no modifications)");
            }
        }
        PlanMode::SourceImmutable => {
            if !tracked {
                bail!("Error: --plan-lock source-immutable requires plan file to be tracked in git");
            }
            if !plan_status.trim().is_empty() {
                bail!("Error: --plan-lock source-immutable requires plan file to be clean (no modifications)");
            }
        }
    }

    let source_sha256 = sha256_hex(content.as_bytes());
    let source_git_oid = if tracked {
        git_blob_oid(project_root, plan_file)?
    } else {
        None
    };

    Ok(ValidatedPlan {
        full_path,
        line_count,
        tracked,
        source_sha256,
        source_git_oid,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn git_blob_oid(project_root: &Path, plan_file: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["-C", project_root.to_str().unwrap_or(".")])
        .args(["rev-parse", &format!("HEAD:{}", plan_file)])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_string()))
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
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", branch),
        ])
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
    fs::copy(source, target)?;
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
    let goal =
        extract_section(plan_content, &["goal", "objective", "purpose"]).unwrap_or_else(|| {
            format!(
                "[To be extracted from plan by Claude in Round 0]\n\nSource plan: {}",
                plan_file
            )
        });
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
