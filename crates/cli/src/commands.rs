//! Command handlers for the Humanize CLI.

mod ask_codex;
mod config;
mod gate;
mod gen_draft;
mod gen_plan;
mod hook_validation;
mod lifecycle;
mod monitor;
mod planning;
mod pr;
mod setup;
mod stop;

pub(crate) use pr::resolve_project_root;

use anyhow::{Context, Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use include_dir::{Dir, include_dir};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::hook_input::{
    HookEventKind, HookInput, HookOutput, get_command, get_file_path, read_hook_input,
};
use crate::{
    CancelCommands, ConfigCommands, GateCommands, HookCommands, MonitorCommands,
    SetupCommands, StopCommands,
};

// Vendored runtime assets live inside the CLI crate so `cargo package` and
// crates.io builds can embed them without depending on workspace-relative paths.
static PROMPT_TEMPLATE_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets/prompt-template");

/// Handle setup commands.
pub fn handle_setup(cmd: SetupCommands) -> Result<()> {
    setup::handle_setup(cmd)
}

/// Handle cancel commands.
pub fn handle_cancel(cmd: CancelCommands) -> Result<()> {
    lifecycle::handle_cancel(cmd)
}

/// Handle monitor commands.
pub fn handle_monitor(cmd: MonitorCommands) -> Result<()> {
    monitor::handle_monitor(cmd)
}

/// Handle gate commands.
pub fn handle_gate(cmd: GateCommands) -> Result<()> {
    gate::handle_gate(cmd)
}

/// Handle stop commands.
pub fn handle_stop(cmd: StopCommands) -> Result<()> {
    stop::handle_stop(cmd)
}

/// Handle ask-codex command.
pub fn handle_ask_codex(prompt: &str, model: &str, effort: &str, timeout: u64) -> Result<()> {
    ask_codex::handle_ask_codex(prompt, model, effort, timeout)
}

/// Handle config commands.
pub fn handle_config(cmd: ConfigCommands) -> Result<()> {
    config::handle_config(cmd)
}

/// Handle gen-draft command.
pub fn handle_gen_draft(input: Option<&str>, title: Option<&str>, stdin: bool) -> Result<()> {
    gen_draft::handle_gen_draft(input, title, stdin)
}

/// Handle gen-plan command.
pub fn handle_gen_plan(
    input: Option<&str>,
    output: Option<&str>,
    draft: Option<&str>,
    prepare_only: bool,
    discussion: bool,
    direct: bool,
    auto_start_rlcr_if_converged: bool,
) -> Result<()> {
    gen_plan::handle_gen_plan(
        input,
        output,
        draft,
        prepare_only,
        discussion,
        direct,
        auto_start_rlcr_if_converged,
    )
}

use hook_validation::{
    validate_bash, validate_edit, validate_plan_file, validate_plan_state_schema, validate_read,
    validate_write,
};

/// Handle hook commands - all read JSON from stdin.
pub fn handle_hook(cmd: HookCommands) -> Result<()> {
    if matches!(cmd, HookCommands::StopFailure) {
        match read_hook_input(false) {
            Ok(input) => stop::handle_stop_failure_hook(&input)?,
            Err(e) => eprintln!("Error: Could not parse StopFailure hook input: {}", e),
        }
        return Ok(());
    }

    let hook_event = match cmd {
        HookCommands::PlanFileValidator => HookEventKind::UserPromptSubmit,
        HookCommands::PostToolUse => HookEventKind::PostToolUse,
        HookCommands::ReadValidator
        | HookCommands::WriteValidator
        | HookCommands::EditValidator
        | HookCommands::BashValidator => HookEventKind::PreToolUse,
        HookCommands::StopFailure => unreachable!("handled above"),
    };

    let require_tool_name = !matches!(cmd, HookCommands::PlanFileValidator);
    let input = match read_hook_input(require_tool_name) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Error: Could not parse hook input: {}", e);
            HookOutput::block(format!("Hook input validation failed: {}", e)).print_for(hook_event);
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
        HookCommands::StopFailure => unreachable!("handled above"),
    };

    result.print_for(hook_event);
    Ok(())
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
        return Err(GitPathCheckError::Failed(output.status.code().unwrap_or(1)));
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        return Err(GitPathCheckError::Failed(1));
    }

    Ok(branch)
}

fn git_path_is_tracked(
    repo_path: &Path,
    path: &str,
) -> std::result::Result<bool, GitPathCheckError> {
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
        return Err(GitPathCheckError::Failed(output.status.code().unwrap_or(1)));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
            ..HookInput::default()
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
            ..HookInput::default()
        });
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };

        assert_eq!(result.decision, "allow");
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
            ..HookInput::default()
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
            "---\ncurrent_round: 0\nmax_iterations: 42\nplan_file: tracked-plan.md\nplan_tracked: true\nplan_mode: source_immutable\nplan_source_path: tracked-plan.md\nplan_source_tracked_at_start: true\nstart_branch: {}\nbase_branch: {}\nreview_started: false\n---\n",
            repo.branch(),
            repo.branch()
        ));
        repo.write_file("tracked-plan.md", "# Plan\nmodified\n");

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        let result = validate_plan_file(&HookInput {
            tool_name: String::new(),
            tool_input: json!({}),
            session_id: None,
            ..HookInput::default()
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
            ..HookInput::default()
        };

        let result = validate_write(&input);
        assert_eq!(result.decision, "block");
        assert!(
            result
                .reason
                .unwrap()
                .contains("PR Loop File Write Blocked")
        );
    }

    #[test]
    fn edit_validator_blocks_pr_loop_readonly_files() {
        let input = HookInput {
            tool_name: "Edit".to_string(),
            tool_input: json!({
                "file_path": "/tmp/project/.humanize/pr-loop/2026-01-18_12-00-00/round-1-pr-check.md"
            }),
            session_id: None,
            ..HookInput::default()
        };

        let result = validate_edit(&input);
        assert_eq!(result.decision, "block");
        assert!(
            result
                .reason
                .unwrap()
                .contains("PR Loop File Write Blocked")
        );
    }

    #[test]
    fn bash_validator_blocks_broad_git_add_patterns() {
        let _guard = ENV_LOCK.lock().unwrap();
        let repo = TestRepo::new();
        repo.write_valid_active_state(1);

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: json!({
                "command": "git add -A"
            }),
            session_id: None,
            ..HookInput::default()
        };

        let result = validate_bash(&input);
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };
        assert_eq!(result.decision, "block");
        assert!(result.reason.unwrap().contains("git add"));
    }

    #[test]
    fn bash_validator_blocks_python_write_to_protected_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let repo = TestRepo::new();
        repo.write_valid_active_state(1);

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: json!({
                "command": "python -c \"open('.humanize/rlcr/2026-01-18_12-00-00/state.md','w').write('x')\""
            }),
            session_id: None,
            ..HookInput::default()
        };

        let result = validate_bash(&input);
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };
        assert_eq!(result.decision, "block");
        assert!(
            result
                .reason
                .unwrap()
                .contains("State File Modification Blocked")
        );
    }

    #[test]
    fn bash_validator_blocks_exec_fd_redirection_to_protected_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let repo = TestRepo::new();
        repo.write_valid_active_state(1);

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: json!({
                "command": "exec 3>.humanize/rlcr/2026-01-18_12-00-00/state.md"
            }),
            session_id: None,
            ..HookInput::default()
        };

        let result = validate_bash(&input);
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };
        assert_eq!(result.decision, "block");
    }

    #[test]
    fn bash_validator_blocks_append_redirect_variants() {
        let _guard = ENV_LOCK.lock().unwrap();
        let repo = TestRepo::new();
        repo.write_valid_active_state(1);

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: json!({
                "command": "printf 'x' &>> .humanize/rlcr/2026-01-18_12-00-00/round-1-summary.md"
            }),
            session_id: None,
            ..HookInput::default()
        };

        let result = validate_bash(&input);
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };
        assert_eq!(result.decision, "block");
    }

    #[test]
    fn bash_validator_blocks_shell_wrapper_for_state_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let repo = TestRepo::new();
        repo.write_valid_active_state(1);

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: json!({
                "command": "sh -c 'rm state.md'"
            }),
            session_id: None,
            ..HookInput::default()
        };

        let result = validate_bash(&input);
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };
        assert_eq!(result.decision, "block");
    }

    #[test]
    fn bash_validator_allows_safe_commands() {
        let _guard = ENV_LOCK.lock().unwrap();
        let repo = TestRepo::new();
        repo.write_valid_active_state(1);

        unsafe { std::env::set_var("CLAUDE_PROJECT_DIR", repo.root()) };
        for command in ["cat state.md", "git status", "ls -la", "grep pattern file"] {
            let input = HookInput {
                tool_name: "Bash".to_string(),
                tool_input: json!({ "command": command }),
                session_id: None,
                ..HookInput::default()
            };
            let result = validate_bash(&input);
            assert_eq!(result.decision, "allow", "command={command}");
        }
        unsafe { std::env::remove_var("CLAUDE_PROJECT_DIR") };
    }

    #[test]
    fn post_tool_use_matches_exact_boundary() {
        assert!(is_setup_invocation(
            "/tmp/setup-rlcr-loop.sh",
            "/tmp/setup-rlcr-loop.sh"
        ));
        assert!(is_setup_invocation(
            "/tmp/setup-rlcr-loop.sh arg",
            "/tmp/setup-rlcr-loop.sh"
        ));
        assert!(is_setup_invocation(
            "/tmp/setup-rlcr-loop.sh\targ",
            "/tmp/setup-rlcr-loop.sh"
        ));
        assert!(is_setup_invocation(
            "\"/tmp/setup-rlcr-loop.sh\" arg",
            "/tmp/setup-rlcr-loop.sh"
        ));

        assert!(!is_setup_invocation(
            "\"/tmp/setup-rlcr-loop.sh\"foo",
            "/tmp/setup-rlcr-loop.sh"
        ));
        assert!(!is_setup_invocation(
            "echo /tmp/setup-rlcr-loop.sh",
            "/tmp/setup-rlcr-loop.sh"
        ));
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
            run(Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(root));
            run(Command::new("git")
                .args(["config", "user.name", "Test"])
                .current_dir(root));

            std::fs::write(root.join("init.txt"), "init\n").unwrap();
            run(Command::new("git")
                .args(["add", "init.txt"])
                .current_dir(root));
            run(Command::new("git")
                .args(["commit", "-q", "-m", "init"])
                .current_dir(root));

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

        fn write_valid_active_state(&self, current_round: u32) {
            self.write_active_state(&format!(
                "---\ncurrent_round: {}\nmax_iterations: 42\ncodex_model: gpt-5.4\ncodex_effort: high\ncodex_timeout: 5400\npush_every_round: false\nfull_review_round: 5\nplan_file: plan.md\nplan_tracked: false\nstart_branch: {}\nbase_branch: {}\nbase_commit: deadbeef\nreview_started: false\nask_codex_question: true\nsession_id:\nagent_teams: false\n---\n",
                current_round,
                self.branch(),
                self.branch()
            ));
        }
    }

    fn run(cmd: &mut Command) {
        let status = cmd.status().unwrap();
        assert!(status.success());
    }
}

/// Check if a line represents an empty session_id field.
fn is_empty_session_id_line(line: &str) -> bool {
    matches!(
        line.trim(),
        "session_id:"
            | "session_id: ~"
            | "session_id: null"
            | "session_id: ''"
            | "session_id: \"\""
    )
}

fn starts_with_boundary(command: &str, prefix: &str) -> bool {
    command
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace))
}

fn is_setup_invocation(command: &str, command_signature: &str) -> bool {
    let quoted = format!("\"{}\"", command_signature);
    starts_with_boundary(command, command_signature) || starts_with_boundary(command, &quoted)
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

    if !is_setup_invocation(&command, command_signature) {
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
            && std::fs::rename(&temp_path, state_file_path).is_err()
        {
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
        && path_lower.ends_with("/state.md")
    {
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
    match state_file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
    {
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
    let single = input
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if single.len() <= max_len {
        single.to_string()
    } else {
        format!("{}...", &single[..max_len.saturating_sub(3)])
    }
}
