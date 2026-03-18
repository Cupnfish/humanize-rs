//! Hook validation logic for Humanize.
//!
//! This module provides validation functions for Claude Code tool hooks,
//! including read, write, edit, bash, and plan file validators.

use crate::fs::{is_protected_state_file, is_round_specific_file};

/// Result of a hook validation.
#[derive(Debug, Clone)]
pub struct HookResult {
    /// Whether the operation is allowed.
    pub allowed: bool,
    /// Reason for blocking (if blocked).
    pub reason: Option<String>,
}

impl HookResult {
    /// Create an allowed result.
    pub fn allowed() -> Self {
        Self {
            allowed: true,
            reason: None,
        }
    }

    /// Create a blocked result with a reason.
    pub fn blocked(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
        }
    }
}

/// Input for read validator hook.
#[derive(Debug, Clone)]
pub struct ReadValidatorInput {
    /// File path being read.
    pub file_path: String,
}

/// Validate a read operation.
pub fn validate_read(input: &ReadValidatorInput) -> HookResult {
    // Block reading round-specific files
    if is_round_specific_file(&input.file_path) {
        return HookResult::blocked(format!(
            "Reading round-specific files is not allowed: {}",
            input.file_path
        ));
    }

    HookResult::allowed()
}

/// Input for write validator hook.
#[derive(Debug, Clone)]
pub struct WriteValidatorInput {
    /// File path being written.
    pub file_path: String,
}

/// Validate a write operation.
pub fn validate_write(input: &WriteValidatorInput) -> HookResult {
    // Block writing to protected state files
    if is_protected_state_file(&input.file_path) {
        return HookResult::blocked(format!(
            "Writing to protected state files is not allowed: {}",
            input.file_path
        ));
    }

    HookResult::allowed()
}

/// Input for edit validator hook.
#[derive(Debug, Clone)]
pub struct EditValidatorInput {
    /// File path being edited.
    pub file_path: String,
    /// Old string being replaced.
    #[allow(dead_code)]
    pub old_string: String,
    /// New string being inserted.
    #[allow(dead_code)]
    pub new_string: String,
}

/// Validate an edit operation.
pub fn validate_edit(input: &EditValidatorInput) -> HookResult {
    // Block editing protected state files
    if is_protected_state_file(&input.file_path) {
        return HookResult::blocked(format!(
            "Editing protected state files is not allowed: {}",
            input.file_path
        ));
    }

    HookResult::allowed()
}

/// Input for bash validator hook.
#[derive(Debug, Clone)]
pub struct BashValidatorInput {
    /// The bash command being executed.
    pub command: String,
}

/// Patterns that indicate file modification.
const FILE_MODIFICATION_PATTERNS: &[&str] = &[
    "rm ", "rm\t", "rmdir", "mv ", "mv\t", "cp ", "> ", ">>", "2>", "| ", " && ", "; ", "`", "$(",
    "chmod", "chown", "mkdir -p",
];

/// Patterns that are generally safe.
const SAFE_COMMAND_PATTERNS: &[&str] = &[
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
    "echo ",
    "ls ",
    "cat ",
    "head ",
    "tail ",
    "grep ",
    "which ",
    "pwd",
];

/// Validate a bash command.
pub fn validate_bash(input: &BashValidatorInput) -> HookResult {
    let cmd = input.command.trim();
    let cmd_lower = cmd.to_lowercase();

    // Check for safe commands first
    for safe_pattern in SAFE_COMMAND_PATTERNS {
        if cmd_lower.starts_with(safe_pattern.to_lowercase().as_str()) {
            return HookResult::allowed();
        }
    }

    // Check for file modification patterns
    for pattern in FILE_MODIFICATION_PATTERNS {
        if cmd.contains(pattern) {
            // Allow if it's a read-only redirection like "grep pattern file | head"
            if *pattern == "| " && is_pipe_to_readonly(&cmd_lower) {
                continue;
            }
            return HookResult::blocked(format!(
                "Command contains file-modifying pattern '{}': {}",
                pattern, cmd
            ));
        }
    }

    HookResult::allowed()
}

/// Check if a pipe is to a read-only command.
fn is_pipe_to_readonly(cmd: &str) -> bool {
    let readonly_commands = [
        "head", "tail", "grep", "wc", "sort", "uniq", "cut", "awk", "sed -n",
    ];
    for ro_cmd in readonly_commands {
        if cmd.contains(&format!("| {}", ro_cmd)) {
            return true;
        }
    }
    false
}

/// Input for plan file validator hook.
#[derive(Debug, Clone)]
pub struct PlanFileValidatorInput {
    /// The plan file path.
    pub plan_file: String,
}

/// Validate a plan file path.
pub fn validate_plan_file(input: &PlanFileValidatorInput) -> HookResult {
    let path = input.plan_file.trim();

    // Block absolute paths
    if path.starts_with('/') {
        return HookResult::blocked(format!("Absolute path not allowed for plan file: {}", path));
    }

    // Block parent traversal
    if path.contains("..") {
        return HookResult::blocked(format!("Parent directory traversal not allowed: {}", path));
    }

    // Block symlinks (would need filesystem check for full implementation)

    HookResult::allowed()
}

/// Input for PostToolUse hook (session handshake).
#[derive(Debug, Clone)]
pub struct PostToolUseInput {
    /// The tool that was used.
    pub tool_name: String,
    /// The tool input (JSON).
    pub tool_input: String,
    /// Path to the pending session ID signal file.
    pub pending_session_file: String,
    /// The session ID to record.
    #[allow(dead_code)]
    pub session_id: String,
}

/// Process PostToolUse hook for session handshake.
pub fn process_post_tool_use(input: &PostToolUseInput) -> HookResult {
    // Only process Bash tool
    if input.tool_name != "Bash" {
        return HookResult::allowed();
    }

    // Check if the pending signal file exists
    let pending_path = std::path::Path::new(&input.pending_session_file);
    if !pending_path.exists() {
        return HookResult::allowed();
    }

    // Read the expected command from the signal file
    let expected_cmd = match std::fs::read_to_string(pending_path) {
        Ok(content) => content.trim().to_string(),
        Err(_) => return HookResult::allowed(),
    };

    // Check if the bash command matches
    if !input.tool_input.contains(&expected_cmd) {
        return HookResult::allowed();
    }

    // This is the setup command - session ID should be recorded
    // (The actual state.md update would be done by the main loop logic)

    // Remove the signal file
    let _ = std::fs::remove_file(pending_path);

    HookResult::allowed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_read_round_file() {
        let input = ReadValidatorInput {
            file_path: ".humanize/rlcr/test/round-1-summary.md".to_string(),
        };
        let result = validate_read(&input);
        assert!(!result.allowed);
    }

    #[test]
    fn test_validate_read_normal_file() {
        let input = ReadValidatorInput {
            file_path: "src/main.rs".to_string(),
        };
        let result = validate_read(&input);
        assert!(result.allowed);
    }

    #[test]
    fn test_validate_write_protected() {
        let input = WriteValidatorInput {
            file_path: ".humanize/rlcr/2026-03-17/state.md".to_string(),
        };
        let result = validate_write(&input);
        assert!(!result.allowed);
    }

    #[test]
    fn test_validate_bash_safe() {
        let input = BashValidatorInput {
            command: "git status".to_string(),
        };
        let result = validate_bash(&input);
        assert!(result.allowed);
    }

    #[test]
    fn test_validate_bash_dangerous() {
        let input = BashValidatorInput {
            command: "rm -rf /".to_string(),
        };
        let result = validate_bash(&input);
        assert!(!result.allowed);
    }

    #[test]
    fn test_validate_plan_file_absolute() {
        let input = PlanFileValidatorInput {
            plan_file: "/etc/passwd".to_string(),
        };
        let result = validate_plan_file(&input);
        assert!(!result.allowed);
    }
}
