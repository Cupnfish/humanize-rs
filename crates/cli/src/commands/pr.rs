use super::*;
use jsongrep::query::{DFAQueryEngine, QueryDFA};
use regex::Regex;
pub(super) struct PrReaction {
    user: String,
    content: String,
    created_at: String,
}

#[derive(Debug, Clone)]
pub(super) struct PrTriggerComment {
    pub(super) id: u64,
    pub(super) created_at: String,
}

#[derive(Debug, Clone)]
pub(super) struct PrReviewEvent {
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
pub(super) struct PrPollOutcome {
    pub(super) comments: Vec<PrReviewEvent>,
    pub(super) timed_out_bots: HashSet<String>,
    pub(super) active_bots: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct PrLookupContext {
    pub(super) pr_number: u32,
    pub(super) repo: String,
}

#[derive(Debug, Clone, Copy)]
struct StopHookPromptConfig {
    compact_large_prompts: bool,
    max_inline_bytes: usize,
}

const STOP_HOOK_COMPACT_PROMPTS_ENV: &str = "HUMANIZE_STOP_HOOK_COMPACT_PROMPTS";
const STOP_HOOK_PROMPT_MAX_INLINE_BYTES_ENV: &str = "HUMANIZE_STOP_HOOK_PROMPT_MAX_INLINE_BYTES";
const STOP_HOOK_PROMPT_DEFAULT_MAX_INLINE_BYTES: usize = 16 * 1024;
const GH_API_MAX_RETRIES: usize = 3;
const GH_API_RETRY_DELAY_SECS: u64 = 2;

#[derive(Debug, Clone, Default)]
struct GhApiValuesOutcome {
    values: Vec<serde_json::Value>,
    failed: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct PrCommentFetchResult {
    pub(super) comments: Vec<PrComment>,
    pub(super) api_failures: usize,
}

pub(crate) fn resolve_project_root() -> Result<PathBuf> {
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

pub(super) fn newest_active_pr_loop(base_dir: &Path) -> Option<PathBuf> {
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
pub(super) struct PrCommitInfo {
    pub(super) latest_commit_sha: String,
    pub(super) latest_commit_at: String,
}

#[derive(Debug, Clone)]
pub(super) struct StartupCaseInfo {
    pub(super) case_num: u32,
    pub(super) comments: Vec<PrComment>,
    pub(super) api_failures: usize,
}

#[derive(Debug, Clone)]
pub(super) struct PrComment {
    id: u64,
    author: String,
    author_type: String,
    created_at: String,
    body: String,
    source: &'static str,
    state: Option<String>,
    path: Option<String>,
    line: Option<u64>,
}

pub(super) fn ensure_command_exists(cmd: &str, message: &str) -> Result<()> {
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

pub(super) fn ensure_gh_auth(project_root: &Path) -> Result<()> {
    let status = Command::new("gh")
        .args(["auth", "status"])
        .current_dir(project_root)
        .status()?;
    if !status.success() {
        bail!("Error: GitHub CLI is not authenticated");
    }
    Ok(())
}

fn jsongrep_values_from_text(json_text: &str, query: &str) -> Result<Vec<serde_json::Value>> {
    let json: serde_json_borrow::Value = serde_json::from_str(json_text)?;
    let dfa = QueryDFA::from_query_str(query)
        .map_err(|err| anyhow::anyhow!("Failed to compile jsongrep query `{query}`: {err}"))?;
    DFAQueryEngine::find_with_dfa(&json, &dfa)
        .into_iter()
        .map(|pointer| {
            serde_json::to_value(pointer.value).context("Failed to serialize jsongrep match")
        })
        .collect()
}

fn jsongrep_first_string_from_text(json_text: &str, query: &str) -> Result<Option<String>> {
    Ok(jsongrep_values_from_text(json_text, query)?
        .into_iter()
        .find_map(|value| match value {
            serde_json::Value::String(text) => Some(text),
            serde_json::Value::Number(number) => Some(number.to_string()),
            serde_json::Value::Bool(flag) => Some(flag.to_string()),
            _ => None,
        }))
}

fn jsongrep_first_u64_from_text(json_text: &str, query: &str) -> Result<Option<u64>> {
    Ok(jsongrep_values_from_text(json_text, query)?
        .into_iter()
        .find_map(|value| value.as_u64()))
}

fn strip_between_delimiter(input: &str, delimiter: &str) -> String {
    input
        .split(delimiter)
        .enumerate()
        .filter_map(|(idx, part)| (idx % 2 == 0).then_some(part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_non_mention_contexts(body: &str) -> String {
    let fenced_backticks = strip_between_delimiter(body, "```");
    let fenced = strip_between_delimiter(&fenced_backticks, "~~~");
    let inline_code = Regex::new(r"`[^`]*`").expect("valid inline code regex");
    let indented_code = Regex::new(r"(?m)^(?: {4}|\t)[^\n]*").expect("valid indented code regex");
    let quoted_lines = Regex::new(r"(?m)^\s*>[^\n]*").expect("valid quote regex");

    let cleaned = inline_code.replace_all(&fenced, " ");
    let cleaned = indented_code.replace_all(&cleaned, " ");
    quoted_lines.replace_all(&cleaned, " ").into_owned()
}

fn bot_mention_regex(bot: &str) -> Result<Regex> {
    Regex::new(&format!(
        r"(?i)(^|[^a-zA-Z0-9_-])@{}($|[^a-zA-Z0-9_-])",
        regex::escape(bot)
    ))
    .map_err(|err| anyhow::anyhow!("Invalid bot mention regex for `{bot}`: {err}"))
}

fn contains_any_bot_mention(body: &str, bots: &[String]) -> bool {
    bots.iter().any(|bot| {
        bot_mention_regex(bot)
            .map(|regex| regex.is_match(body))
            .unwrap_or(false)
    })
}

fn contains_all_bot_mentions(body: &str, bots: &[String]) -> bool {
    let cleaned = strip_non_mention_contexts(body);
    bots.iter().all(|bot| {
        bot_mention_regex(bot)
            .map(|regex| regex.is_match(&cleaned))
            .unwrap_or(false)
    })
}

fn is_bot_author(author_type: &str, author: &str) -> bool {
    author_type == "Bot" || author.ends_with("[bot]")
}

fn repo_from_pr_url(url: &str) -> Option<String> {
    Regex::new(r"^https?://[^/]+/([^/]+/[^/]+)/pull/")
        .ok()?
        .captures(url)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().to_string()))
}

pub(super) fn gh_current_user(project_root: &Path) -> Result<String> {
    let output = gh_output(project_root, &["api", "user"])?;
    if !output.status.success() {
        bail!("Error: Failed to get current GitHub user");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    jsongrep_first_string_from_text(&stdout, "login")?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Error: Failed to parse current GitHub user"))
}

pub(super) fn gh_detect_pr_context(project_root: &Path) -> Result<PrLookupContext> {
    let output = gh_output(project_root, &["pr", "view", "--json", "number,url"])?;
    if !output.status.success() {
        bail!("Error: No pull request found for the current branch");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let current_repo = gh_current_repo_json(project_root)?;
    let pr_number = jsongrep_first_u64_from_text(&stdout, "number")?
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| anyhow::anyhow!("Error: Invalid PR number from gh CLI"))?;
    let repo = jsongrep_first_string_from_text(&stdout, "url")?
        .and_then(|url| repo_from_pr_url(&url))
        .unwrap_or(current_repo);
    Ok(PrLookupContext { pr_number, repo })
}

pub(super) fn gh_detect_pr_context_for_branch(
    project_root: &Path,
    start_branch: &str,
) -> Result<PrLookupContext> {
    if let Ok(context) = gh_detect_pr_context(project_root) {
        return Ok(context);
    }

    let current_repo = gh_current_repo_json(project_root)?;
    let Some(parent_repo) = gh_parent_repo(project_root)? else {
        bail!("Error: No pull request found for branch `{start_branch}`");
    };
    let fork_owner = current_repo
        .split('/')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Error: Failed to determine fork owner"))?;
    let qualified_branch = format!("{}:{}", fork_owner, start_branch);
    let output = gh_output(
        project_root,
        &[
            "pr",
            "view",
            "--repo",
            &parent_repo,
            &qualified_branch,
            "--json",
            "number",
        ],
    )?;
    if !output.status.success() {
        bail!("Error: No pull request found for branch `{start_branch}`");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pr_number = jsongrep_first_u64_from_text(&stdout, "number")?
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| anyhow::anyhow!("Error: Invalid PR number from gh CLI"))?;
    Ok(PrLookupContext {
        pr_number,
        repo: parent_repo,
    })
}

#[allow(dead_code)]
pub(super) fn build_active_bots_from_flags(claude: bool, codex: bool) -> Vec<String> {
    let mut bots = Vec::new();
    if claude {
        bots.push("claude".to_string());
    }
    if codex {
        bots.push("codex".to_string());
    }
    bots
}

pub(super) fn build_bot_mention_string(bots: &[String]) -> String {
    bots.iter()
        .map(|bot| format!("@{}", bot))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn bot_author(bot: &str) -> &str {
    match bot {
        "codex" => "chatgpt-codex-connector[bot]",
        "claude" => "claude[bot]",
        _ => bot,
    }
}

pub(super) fn gh_fetch_comments_detailed(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
) -> PrCommentFetchResult {
    let mut api_failures = 0usize;
    let mut comments = Vec::new();

    let issue = gh_fetch_comment_source_tolerant(
        project_root,
        &format!("repos/{}/issues/{}/comments", repo, pr_number),
        "issue_comment",
        "issue comments",
    );
    comments.extend(issue.comments);
    api_failures += issue.api_failures;

    let review = gh_fetch_comment_source_tolerant(
        project_root,
        &format!("repos/{}/pulls/{}/comments", repo, pr_number),
        "review_comment",
        "PR review comments",
    );
    comments.extend(review.comments);
    api_failures += review.api_failures;

    let reviews = gh_fetch_review_source_tolerant(
        project_root,
        &format!("repos/{}/pulls/{}/reviews", repo, pr_number),
        "PR reviews",
    );
    comments.extend(reviews.comments);
    api_failures += reviews.api_failures;

    let mut seen = HashSet::new();
    comments.retain(|comment| seen.insert(comment.id));
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    PrCommentFetchResult {
        comments,
        api_failures,
    }
}

fn gh_fetch_comment_source_tolerant(
    project_root: &Path,
    endpoint: &str,
    source: &'static str,
    description: &str,
) -> PrCommentFetchResult {
    let outcome = gh_api_values_tolerant(project_root, endpoint, description);
    let comments = outcome
        .values
        .into_iter()
        .filter_map(|value| {
            let id = value.get("id").and_then(|v| v.as_u64())?;
            let author = value
                .get("user")
                .and_then(|v| v.get("login"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let author_type = value
                .get("user")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| {
                    if author.ends_with("[bot]") {
                        "Bot".to_string()
                    } else {
                        "User".to_string()
                    }
                });
            Some(PrComment {
                id,
                author,
                author_type,
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
                state: None,
                path: value
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string),
                line: value
                    .get("line")
                    .or_else(|| value.get("original_line"))
                    .and_then(|v| v.as_u64()),
            })
        })
        .collect();
    PrCommentFetchResult {
        comments,
        api_failures: usize::from(outcome.failed),
    }
}

fn gh_fetch_review_source_tolerant(
    project_root: &Path,
    endpoint: &str,
    description: &str,
) -> PrCommentFetchResult {
    let outcome = gh_api_values_tolerant(project_root, endpoint, description);
    let comments = outcome
        .values
        .into_iter()
        .filter_map(|value| {
            let id = value.get("id").and_then(|v| v.as_u64())?;
            let state = value
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let body = value
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let author = value
                .get("user")
                .and_then(|v| v.get("login"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let author_type = value
                .get("user")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| {
                    if author.ends_with("[bot]") {
                        "Bot".to_string()
                    } else {
                        "User".to_string()
                    }
                });
            Some(PrComment {
                id,
                author: value
                    .get("user")
                    .and_then(|v| v.get("login"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                author_type,
                created_at: value
                    .get("submitted_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                body: if body.is_empty() {
                    format!("[Review state: {}]", state)
                } else {
                    body.to_string()
                },
                source: "pr_review",
                state: Some(state.to_string()),
                path: None,
                line: None,
            })
        })
        .collect();
    PrCommentFetchResult {
        comments,
        api_failures: usize::from(outcome.failed),
    }
}

pub(super) fn gh_startup_case(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
    bots: &[String],
    latest_commit_at: &str,
) -> Result<StartupCaseInfo> {
    let fetch = gh_fetch_comments_detailed(project_root, repo, pr_number);
    let comments = fetch.comments;
    let case_num = compute_startup_case_from_comments(&comments, bots, latest_commit_at);
    Ok(StartupCaseInfo {
        case_num,
        comments,
        api_failures: fetch.api_failures,
    })
}

pub(super) fn compute_startup_case_from_comments(
    comments: &[PrComment],
    bots: &[String],
    latest_commit_at: &str,
) -> u32 {
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

    case_num
}

fn render_pr_comment_block(comment: &PrComment, heading: &str) -> String {
    let mut out = String::new();
    out.push_str(heading);
    out.push_str("\n\n");
    out.push_str(&format!(
        "- **Type**: {}\n",
        comment.source.replace('_', " ")
    ));
    out.push_str(&format!("- **Time**: {}\n", comment.created_at));
    if let Some(path) = &comment.path {
        if let Some(line) = comment.line {
            out.push_str(&format!("- **File**: `{path}` (line {line})\n"));
        } else {
            out.push_str(&format!("- **File**: `{path}`\n"));
        }
    }
    if let Some(state) = &comment.state {
        out.push_str(&format!("- **Status**: {state}\n"));
    }
    out.push('\n');
    out.push_str(&comment.body);
    out.push_str("\n\n---\n");
    out
}

pub(super) fn format_initial_pr_comments(
    pr_number: u32,
    repo: &str,
    active_bots: &[String],
    comments: &[PrComment],
    api_failures: usize,
) -> String {
    let mut out = format!(
        "# PR Comments for #{}\n\nFetched at: {}\nRepository: {}\n\n---\n\n",
        pr_number,
        now_utc_string(),
        repo
    );

    if comments.is_empty() {
        out.push_str("*No comments found.*\n\n---\n\nThis PR has no review comments yet from the monitored bots.\n");
        if api_failures > 0 {
            out.push_str("\n**Warning:** Some API calls failed. Comments may be incomplete.\n");
        }
        out.push_str("\n---\n\n*End of comments*\n");
        return out;
    }

    let mut sorted_comments = comments
        .iter()
        .filter(|comment| !comment.created_at.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    sorted_comments.sort_by(|a, b| {
        let a_bot = is_bot_author(&a.author_type, &a.author);
        let b_bot = is_bot_author(&b.author_type, &b.author);
        a_bot
            .cmp(&b_bot)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });

    out.push_str("## Human Comments\n\n");
    let human_comments = sorted_comments
        .iter()
        .filter(|comment| !is_bot_author(&comment.author_type, &comment.author))
        .collect::<Vec<_>>();
    if human_comments.is_empty() {
        out.push_str("*No human comments.*\n\n");
    } else {
        for comment in human_comments {
            out.push_str(&render_pr_comment_block(
                comment,
                &format!("### Comment from {}", comment.author),
            ));
        }
        out.push('\n');
    }

    if !active_bots.is_empty() {
        out.push_str("## Bot Comments (Grouped by Bot)\n\n");
        for bot in active_bots {
            let author = bot_author(bot);
            out.push_str(&format!("### Comments from {}\n\n", author));
            let bot_comments = sorted_comments
                .iter()
                .filter(|comment| comment.author == author)
                .collect::<Vec<_>>();
            if bot_comments.is_empty() {
                out.push_str("*No comments from this bot.*\n\n");
                continue;
            }
            for comment in bot_comments {
                out.push_str(&render_pr_comment_block(comment, "#### Comment"));
            }
            out.push('\n');
        }
    } else {
        out.push_str("## Bot Comments\n\n");
        for comment in sorted_comments
            .iter()
            .filter(|comment| is_bot_author(&comment.author_type, &comment.author))
        {
            out.push_str(&render_pr_comment_block(
                comment,
                &format!("### Comment from {}", comment.author),
            ));
        }
    }

    if api_failures > 0 {
        out.push_str("\n**Warning:** Some API calls failed. Comments may be incomplete.\n");
    }

    out.push_str("\n---\n\n*End of comments*\n");
    out
}

pub(super) fn gh_find_latest_user_comment(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
    user: &str,
) -> Result<Option<(u64, String)>> {
    let values = gh_api_values(
        project_root,
        &format!("repos/{}/issues/{}/comments", repo, pr_number),
    )
    .unwrap_or_default();
    let latest = values
        .into_iter()
        .filter(|value| {
            value
                .get("user")
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

pub(super) fn build_pr_goal_tracker(
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

pub(super) fn build_pr_round_0_prompt(
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

pub(super) fn template_vars(pairs: &[(&str, String)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

pub(super) fn embedded_template_contents(template_name: &str) -> Option<&'static str> {
    PROMPT_TEMPLATE_ASSETS
        .get_file(template_name)
        .and_then(|file| file.contents_utf8())
}

pub(super) fn render_template_or_fallback(
    template_name: &str,
    fallback: &str,
    vars: &[(&str, String)],
) -> String {
    if let Some(template) = embedded_template_contents(template_name) {
        humanize_core::template::render_template(template, &template_vars(vars))
    } else {
        humanize_core::template::render_template(fallback, &template_vars(vars))
    }
}

const MAX_LOOP_FILE_LINES: usize = 2000;

pub(super) fn git_status_porcelain_cached(project_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["-C", project_root.to_str().unwrap_or(".")])
        .args(["status", "--porcelain"])
        .output()?;
    if !output.status.success() {
        bail!("git status --porcelain failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(super) fn git_non_humanize_status_lines(status: &str) -> Vec<String> {
    status
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.contains(".humanize"))
        .map(ToString::to_string)
        .collect()
}

pub(super) fn git_changed_paths(status: &str) -> Vec<String> {
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

pub(super) fn detect_large_changed_files(
    project_root: &Path,
    status: &str,
) -> Vec<(String, usize, &'static str)> {
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

pub(super) fn build_large_files_reason(large_files: &[(String, usize, &'static str)]) -> String {
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

pub(super) fn build_git_not_clean_reason(git_issues: &str, special_notes: &str) -> String {
    render_template_or_fallback(
        "block/git-not-clean.md",
        "# Git Not Clean\n\nYou are trying to stop, but you have {{GIT_ISSUES}}.\n{{SPECIAL_NOTES}}\nCommit your changes and try again.",
        &[
            ("GIT_ISSUES", git_issues.to_string()),
            ("SPECIAL_NOTES", special_notes.to_string()),
        ],
    )
}

pub(super) fn rlcr_goal_tracker_update_section(loop_dir: &Path) -> String {
    render_template_or_fallback(
        "codex/goal-tracker-update-section.md",
        "## Goal Tracker Updates\nIf Claude's summary includes a Goal Tracker Update Request section, apply the requested changes to {{GOAL_TRACKER_FILE}}.",
        &[(
            "GOAL_TRACKER_FILE",
            loop_dir.join("goal-tracker.md").display().to_string(),
        )],
    )
}

pub(super) fn is_full_alignment_round(current_round: u32, full_review_round: u32) -> bool {
    let normalized = if full_review_round < 2 {
        5
    } else {
        full_review_round
    };
    current_round % normalized == normalized - 1
}

pub(super) fn detect_open_question(review_content: &str) -> bool {
    review_content
        .lines()
        .any(|line| line.len() < 40 && line.contains("Open Question"))
}

pub(super) fn build_impl_review_prompt(
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

fn stop_hook_prompt_config() -> StopHookPromptConfig {
    StopHookPromptConfig {
        compact_large_prompts: std::env::var(STOP_HOOK_COMPACT_PROMPTS_ENV)
            .map(|value| {
                let normalized = value.trim().to_ascii_lowercase();
                !matches!(normalized.as_str(), "0" | "false" | "off")
            })
            .unwrap_or(true),
        max_inline_bytes: std::env::var(STOP_HOOK_PROMPT_MAX_INLINE_BYTES_ENV)
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(STOP_HOOK_PROMPT_DEFAULT_MAX_INLINE_BYTES),
    }
}

fn maybe_compact_stop_hook_prompt<F>(inline_prompt: String, compact_builder: F) -> String
where
    F: FnOnce() -> String,
{
    let config = stop_hook_prompt_config();
    if !config.compact_large_prompts || inline_prompt.len() <= config.max_inline_bytes {
        return inline_prompt;
    }

    // Keep stop-hook prompt bodies below the risky size window until Claude Code fixes
    // the large-stdout Stop-hook regression: https://github.com/anthropics/claude-code/issues/37135
    compact_builder()
}

pub(super) fn build_next_round_prompt(
    loop_dir: &Path,
    state: &humanize_core::state::State,
    review_content: &str,
) -> String {
    let next_round = state.current_round + 1;
    let next_summary_file = loop_dir.join(format!("round-{}-summary.md", next_round));
    let full_alignment = is_full_alignment_round(state.current_round, state.full_review_round);
    let goal_tracker_file = loop_dir.join("goal-tracker.md");
    let review_result_file =
        loop_dir.join(format!("round-{}-review-result.md", state.current_round));

    let inline_prompt = render_template_or_fallback(
        "claude/next-round-prompt.md",
        "Your work is not finished. Read and execute the below with ultrathink.\n\n## Original Implementation Plan\n\n@{{PLAN_FILE}}\n\nBelow is Codex's review result:\n{{REVIEW_CONTENT}}\n\n## Goal Tracker Reference\n@{{GOAL_TRACKER_FILE}}\n",
        &[
            ("PLAN_FILE", state.plan_file.clone()),
            ("REVIEW_CONTENT", review_content.to_string()),
            ("GOAL_TRACKER_FILE", goal_tracker_file.display().to_string()),
        ],
    );
    let mut prompt = maybe_compact_stop_hook_prompt(inline_prompt, || {
        render_template_or_fallback(
            "claude/next-round-prompt-compact.md",
            "Your work is not finished. Read and execute the below with ultrathink.\n\n## Original Implementation Plan\n\n@{{PLAN_FILE}}\n\n## Codex Review Result\n\nThe full Codex review is in:\n@{{REVIEW_RESULT_FILE}}\n\nRead that file carefully before making changes.\n\n## Goal Tracker Reference\n@{{GOAL_TRACKER_FILE}}\n",
            &[
                ("PLAN_FILE", state.plan_file.clone()),
                (
                    "REVIEW_RESULT_FILE",
                    review_result_file.display().to_string(),
                ),
                ("GOAL_TRACKER_FILE", goal_tracker_file.display().to_string()),
            ],
        )
    });

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

pub(super) fn build_review_phase_fix_prompt(
    review_content: &str,
    review_result_file: &Path,
    summary_file: &Path,
) -> String {
    let inline_prompt = render_template_or_fallback(
        "claude/review-phase-prompt.md",
        "# Code Review Findings\n\n{{REVIEW_CONTENT}}\n\nWrite your summary to: `{{SUMMARY_FILE}}`",
        &[
            ("REVIEW_CONTENT", review_content.to_string()),
            ("SUMMARY_FILE", summary_file.display().to_string()),
        ],
    );
    maybe_compact_stop_hook_prompt(inline_prompt, || {
        render_template_or_fallback(
            "claude/review-phase-prompt-compact.md",
            "# Code Review Findings\n\nThe full Codex review findings are in:\n@{{REVIEW_RESULT_FILE}}\n\nRead that file carefully and address every issue before continuing.\n\nWrite your summary to: `{{SUMMARY_FILE}}`",
            &[
                (
                    "REVIEW_RESULT_FILE",
                    review_result_file.display().to_string(),
                ),
                ("SUMMARY_FILE", summary_file.display().to_string()),
            ],
        )
    })
}

pub(super) fn build_review_phase_audit_prompt(review_round: u32, base_branch: &str) -> String {
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

pub(super) fn build_finalize_phase_prompt(
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

pub(super) fn parse_json_value_stream(bytes: &[u8]) -> Result<Vec<serde_json::Value>> {
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

pub(super) fn gh_output(project_root: &Path, args: &[&str]) -> Result<std::process::Output> {
    Ok(Command::new("gh")
        .args(args)
        .current_dir(project_root)
        .output()?)
}

fn gh_api_values_with_retry(
    project_root: &Path,
    endpoint: &str,
    description: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut last_error: Option<String> = None;

    for attempt in 1..=GH_API_MAX_RETRIES {
        let output = gh_output(project_root, &["api", endpoint, "--paginate"])?;
        if output.status.success() {
            return parse_json_value_stream(&output.stdout);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        last_error = Some(stderr.clone());
        if attempt < GH_API_MAX_RETRIES {
            eprintln!(
                "Warning: Failed to fetch {description} (attempt {attempt}/{GH_API_MAX_RETRIES}), retrying in {GH_API_RETRY_DELAY_SECS}s..."
            );
            sleep(Duration::from_secs(GH_API_RETRY_DELAY_SECS));
        }
    }

    bail!(
        "GitHub API failed for {} after {} attempts: {}",
        endpoint,
        GH_API_MAX_RETRIES,
        last_error.unwrap_or_default()
    );
}

fn gh_api_values_tolerant(
    project_root: &Path,
    endpoint: &str,
    description: &str,
) -> GhApiValuesOutcome {
    match gh_api_values_with_retry(project_root, endpoint, description) {
        Ok(values) => GhApiValuesOutcome {
            values,
            failed: false,
        },
        Err(err) => {
            eprintln!(
                "WARNING: Failed to fetch {description} after {GH_API_MAX_RETRIES} attempts: {err}"
            );
            GhApiValuesOutcome {
                values: Vec::new(),
                failed: true,
            }
        }
    }
}

pub(super) fn gh_api_values(project_root: &Path, endpoint: &str) -> Result<Vec<serde_json::Value>> {
    gh_api_values_with_retry(project_root, endpoint, endpoint)
}

pub(super) fn gh_current_repo_json(project_root: &Path) -> Result<String> {
    let output = gh_output(project_root, &["repo", "view", "--json", "owner,name"])?;
    if !output.status.success() {
        bail!("Error: Failed to get current repository");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let owner = jsongrep_first_string_from_text(&stdout, "owner.login")?.unwrap_or_default();
    let name = jsongrep_first_string_from_text(&stdout, "name")?.unwrap_or_default();
    if owner.is_empty() || name.is_empty() {
        bail!("Error: Failed to parse current repository");
    }
    Ok(format!("{}/{}", owner, name))
}

pub(super) fn gh_parent_repo(project_root: &Path) -> Result<Option<String>> {
    let output = gh_output(project_root, &["repo", "view", "--json", "parent"])?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let owner = jsongrep_first_string_from_text(&stdout, "parent.owner.login")?.unwrap_or_default();
    let name = jsongrep_first_string_from_text(&stdout, "parent.name")?.unwrap_or_default();
    if owner.is_empty() || name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!("{}/{}", owner, name)))
    }
}

pub(super) fn gh_repo_contains_pr(project_root: &Path, repo: &str, pr_number: u32) -> bool {
    gh_output(
        project_root,
        &[
            "pr",
            "view",
            &pr_number.to_string(),
            "--repo",
            repo,
            "--json",
            "number",
        ],
    )
    .map(|output| output.status.success())
    .unwrap_or(false)
}

pub(super) fn gh_resolve_pr_repo(project_root: &Path, pr_number: u32) -> Result<String> {
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

pub(super) fn gh_pr_state_in_repo(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
) -> Result<String> {
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    jsongrep_first_string_from_text(&stdout, "state")?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Error: Failed to parse PR state"))
}

pub(super) fn gh_pr_commit_info_in_repo(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
) -> Result<PrCommitInfo> {
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    let latest_commit_sha =
        jsongrep_first_string_from_text(&stdout, "headRefOid")?.unwrap_or_default();
    let latest_commit_at = jsongrep_values_from_text(&stdout, "commits.[*].committedDate")?
        .into_iter()
        .filter_map(|value| value.as_str().map(ToString::to_string))
        .max()
        .unwrap_or_default();
    Ok(PrCommitInfo {
        latest_commit_sha,
        latest_commit_at,
    })
}

pub(super) fn map_author_to_bot(author: &str) -> Option<String> {
    match author {
        "chatgpt-codex-connector[bot]" => Some("codex".to_string()),
        "claude[bot]" => Some("claude".to_string()),
        other if other.ends_with("[bot]") => Some(other.trim_end_matches("[bot]").to_string()),
        _ => None,
    }
}

pub(super) fn parse_iso_timestamp_epoch(timestamp: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.timestamp())
}

pub(super) fn now_utc_string() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub(super) fn pr_requires_trigger(
    current_round: u32,
    startup_case: &str,
    new_commits_detected: bool,
) -> bool {
    if current_round > 0 {
        true
    } else if new_commits_detected {
        true
    } else {
        matches!(startup_case, "4" | "5")
    }
}

pub(super) fn sanitize_bot_list(bots: &[String]) -> Vec<String> {
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

pub(super) fn gh_find_existing_trigger_comment(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
    current_user: &str,
    active_bots: &[String],
    latest_commit_at: &str,
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

        let created_at = value
            .get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if !latest_commit_at.is_empty() && created_at.as_str() <= latest_commit_at {
            continue;
        }

        let body = value
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !contains_all_bot_mentions(body, active_bots) {
            continue;
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

pub(super) fn gh_detect_trigger_comment(
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
        let body = value
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !contains_any_bot_mention(body, configured_bots) {
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

pub(super) fn gh_issue_reactions(
    project_root: &Path,
    repo: &str,
    issue_number: u32,
) -> Result<Vec<PrReaction>> {
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

pub(super) fn gh_comment_reactions(
    project_root: &Path,
    repo: &str,
    comment_id: &str,
) -> Result<Vec<PrReaction>> {
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

pub(super) fn gh_find_codex_thumbsup(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
    after_timestamp: Option<&str>,
) -> Result<Option<PrReaction>> {
    let mut matches = match gh_issue_reactions(project_root, repo, pr_number) {
        Ok(reactions) => reactions,
        Err(err) => {
            eprintln!("Warning: Failed to fetch Codex reactions: {err}");
            return Ok(None);
        }
    }
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

pub(super) fn gh_wait_for_claude_eyes(
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
            if let Some(reaction) = reactions
                .into_iter()
                .find(|reaction| reaction.user == "claude[bot]" && reaction.content == "eyes")
            {
                return Ok(Some(reaction));
            }
        }
    }
    Ok(None)
}

pub(super) fn gh_fetch_review_events(
    project_root: &Path,
    repo: &str,
    pr_number: u32,
) -> Result<Vec<PrReviewEvent>> {
    let issue_values = gh_api_values_tolerant(
        project_root,
        &format!("repos/{}/issues/{}/comments", repo, pr_number),
        "issue comments",
    )
    .values;
    let review_comment_values = gh_api_values_tolerant(
        project_root,
        &format!("repos/{}/pulls/{}/comments", repo, pr_number),
        "PR review comments",
    )
    .values;
    let review_values = gh_api_values_tolerant(
        project_root,
        &format!("repos/{}/pulls/{}/reviews", repo, pr_number),
        "PR reviews",
    )
    .values;

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
            path: value
                .get("path")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
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
        let body = value
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
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

pub(super) fn filter_review_events(
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
        .filter(|event| {
            event.created_at.as_str() >= after_timestamp && authors.contains(&event.author)
        })
        .collect()
}

pub(super) fn format_pr_review_comments_markdown(
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
            out.push_str(&format!(
                "- **Type**: {}\n",
                comment.source.replace('_', " ")
            ));
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

pub(super) fn pr_last_marker(check_content: &str) -> String {
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

pub(super) fn parse_pr_bot_statuses(check_content: &str) -> HashMap<String, String> {
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

pub(super) fn count_pr_issues_found(check_content: &str) -> u32 {
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

pub(super) fn update_pr_goal_tracker(
    goal_tracker_path: &Path,
    round: u32,
    bot_results: Option<(&str, u32, u32)>,
) -> Result<()> {
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
    let total_found = if has_summary_row {
        current_found
    } else {
        current_found + new_issues
    };
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
        original.replace(
            &format!(
                "- Total Issues Found: {}\n- Total Issues Resolved: {}\n- Remaining: {}",
                current_found,
                current_resolved,
                current_found.saturating_sub(current_resolved)
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

pub(super) fn build_pr_feedback_markdown(
    next_round: u32,
    max_iterations: u32,
    pr_number: u32,
    loop_dir: &Path,
    active_bots: &[String],
    check_file: &Path,
    check_content: &str,
) -> String {
    let bot_mentions = build_bot_mention_string(active_bots);
    let inline_prompt = format!(
        "# PR Loop Feedback (Round {})\n\n## Bot Review Analysis\n\n{}\n\n---\n\n## Your Task\n\nAddress the issues identified above:\n\n1. Read and understand each issue\n2. Make the necessary code changes\n3. Commit and push your changes\n4. Comment on the PR to trigger re-review:\n   ```bash\ngh pr comment {} --body \"{} please review the latest changes\"\n   ```\n5. Write your resolution summary to: {}\n\n---\n\n**Remaining active bots:** {}\n**Round:** {} of {}\n",
        next_round,
        check_content.trim(),
        pr_number,
        bot_mentions,
        loop_dir
            .join(format!("round-{}-pr-resolve.md", next_round))
            .display(),
        active_bots.join(", "),
        next_round,
        max_iterations
    );
    maybe_compact_stop_hook_prompt(inline_prompt, || {
        format!(
            "# PR Loop Feedback (Round {})\n\n## Bot Review Analysis\n\nThe full Codex bot-review analysis is in:\n@{}\n\nRead that file carefully and address every remaining issue before continuing.\n\n---\n\n## Your Task\n\n1. Read and understand each issue\n2. Make the necessary code changes\n3. Commit and push your changes\n4. Comment on the PR to trigger re-review:\n   ```bash\ngh pr comment {} --body \"{} please review the latest changes\"\n   ```\n5. Write your resolution summary to: {}\n\n---\n\n**Remaining active bots:** {}\n**Round:** {} of {}\n",
            next_round,
            check_file.display(),
            pr_number,
            bot_mentions,
            loop_dir
                .join(format!("round-{}-pr-resolve.md", next_round))
                .display(),
            active_bots.join(", "),
            next_round,
            max_iterations
        )
    })
}

pub(super) fn current_unix_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(super) fn git_stdout(project_root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(["-C", project_root.to_str().unwrap_or(".")])
        .args(args)
        .output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(super) fn git_stdout_optional(project_root: &Path, args: &[&str]) -> Option<String> {
    git_stdout(project_root, args)
        .ok()
        .filter(|value| !value.is_empty())
}

pub(super) fn pr_ahead_count(project_root: &Path, repo: &str, pr_number: u32) -> Result<u32> {
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

    let current_branch = git_current_branch(project_root)
        .map_err(|_| anyhow::anyhow!("git branch lookup failed"))?;
    let local_head = git_stdout_optional(project_root, &["rev-parse", "HEAD"]).unwrap_or_default();

    if git_stdout(project_root, &["rev-parse", "--abbrev-ref", "@{u}"]).is_err() {
        let remote_ref = format!("origin/{}", current_branch);
        let remote_head = git_stdout_optional(project_root, &["rev-parse", &remote_ref]);
        if let Some(remote_head) = remote_head {
            if !local_head.is_empty() && local_head != remote_head {
                let count = git_stdout_optional(
                    project_root,
                    &["rev-list", "--count", &format!("{}..HEAD", remote_ref)],
                )
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0);
                return Ok(count);
            }
        } else {
            let commit_info = gh_pr_commit_info_in_repo(project_root, repo, pr_number)?;
            if !commit_info.latest_commit_sha.is_empty()
                && local_head != commit_info.latest_commit_sha
            {
                let count = git_stdout_optional(
                    project_root,
                    &[
                        "rev-list",
                        "--count",
                        &format!("{}..HEAD", commit_info.latest_commit_sha),
                    ],
                )
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(1);
                return Ok(count.max(1));
            }
        }
        return Ok(0);
    }

    Ok(
        git_stdout_optional(project_root, &["rev-list", "--count", "@{u}..HEAD"])
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0),
    )
}

pub(super) fn pr_poll_reviews(
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
            if gh_find_codex_thumbsup(project_root, repo, pr_number, Some(reaction_after))?
                .is_some()
            {
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

pub(super) fn run_pr_codex_review(
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

    let result = humanize_core::codex::run_exec(&prompt, &options)
        .map_err(|err| anyhow::anyhow!("Codex failed to validate bot reviews: {}", err))?;
    fs::write(check_file, &result.stdout)?;
    Ok(result.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_all_bot_mentions_ignores_code_and_quotes() {
        let body = r#"
```text
@claude
@codex
```
> @claude
Real request: @claude please review
Inline `@codex` should not count
Also real: @codex please review
"#;
        assert!(contains_all_bot_mentions(
            body,
            &["claude".to_string(), "codex".to_string()]
        ));
    }

    #[test]
    fn contains_all_bot_mentions_rejects_partial_or_suffix_matches() {
        let body = "@claude-dev please review support@codex.io";
        assert!(!contains_all_bot_mentions(
            body,
            &["claude".to_string(), "codex".to_string()]
        ));
        assert!(!contains_any_bot_mention(
            body,
            &["claude".to_string(), "codex".to_string()]
        ));
    }
}
