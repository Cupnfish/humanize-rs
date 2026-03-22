//! Hook input parsing for Claude Code hooks.
//!
//! Claude Code hooks receive JSON on stdin with this structure:
//! ```json
//! {
//!   "tool_name": "Read",
//!   "tool_input": {
//!     "file_path": "/path/to/file"
//!   },
//!   "session_id": "abc123"
//! }
//! ```

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::{self, Read};

/// Maximum JSON nesting depth allowed.
const MAX_JSON_DEPTH: usize = 30;

/// Input from Claude Code hooks.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct HookInput {
    /// The tool being invoked (e.g., "Read", "Write", "Bash")
    #[serde(default)]
    pub tool_name: String,
    /// The tool input parameters
    #[serde(default)]
    pub tool_input: serde_json::Value,
    /// Session identifier
    #[serde(default)]
    pub session_id: Option<String>,
    /// The last assistant message, when provided by Claude Code for Stop/StopFailure events
    #[serde(default)]
    pub last_assistant_message: Option<String>,
    /// StopFailure error kind such as rate_limit / billing_error / authentication_failed
    #[serde(default)]
    pub error: Option<String>,
    /// Tool output (for PostToolUse hooks)
    #[serde(default)]
    #[allow(dead_code)]
    pub tool_output: Option<serde_json::Value>,
    /// Tool result (for PostToolUse hooks)
    #[serde(default)]
    #[allow(dead_code)]
    pub tool_result: Option<String>,
}

/// Output from hook validators.
#[derive(Debug, Clone, Serialize)]
pub struct HookOutput {
    /// Decision: "allow" or "block"
    pub decision: String,
    /// Reason for blocking (if blocked)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl HookOutput {
    /// Create an allow decision.
    pub fn allow() -> Self {
        Self {
            decision: "allow".to_string(),
            reason: None,
        }
    }

    /// Create a block decision with a reason.
    pub fn block(reason: impl Into<String>) -> Self {
        Self {
            decision: "block".to_string(),
            reason: Some(reason.into()),
        }
    }

    /// Render in the event-specific hook schema expected by Claude Code.
    pub fn render_for(&self, event: HookEventKind) -> Option<serde_json::Value> {
        let Some(reason) = &self.reason else {
            return None;
        };

        Some(match event {
            HookEventKind::UserPromptSubmit => json!({
                "decision": "block",
                "reason": reason,
            }),
            HookEventKind::PreToolUse => json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": reason,
                }
            }),
            HookEventKind::PostToolUse => json!({
                "decision": "block",
                "reason": reason,
            }),
        })
    }

    /// Output in the event-specific hook schema expected by Claude Code.
    pub fn print_for(&self, event: HookEventKind) {
        if let Some(value) = self.render_for(event) {
            println!("{}", serde_json::to_string(&value).unwrap());
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum HookEventKind {
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
}

/// Read and parse hook input from stdin.
pub fn read_hook_input(require_tool_name: bool) -> Result<HookInput> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Check for null bytes
    if input.contains('\0') {
        bail!("Error: Input contains null bytes");
    }

    // Validate JSON depth
    if is_deeply_nested(&input, MAX_JSON_DEPTH) {
        bail!(
            "Error: JSON structure exceeds maximum depth of {}",
            MAX_JSON_DEPTH
        );
    }

    // Parse JSON
    let hook_input: HookInput = serde_json::from_str(&input)?;

    // Validate tool_name is present when required.
    if require_tool_name && hook_input.tool_name.is_empty() {
        bail!("Error: Missing required field: tool_name");
    }

    Ok(hook_input)
}

/// Check if JSON is deeply nested (potential DoS).
fn is_deeply_nested(json: &str, max_depth: usize) -> bool {
    let mut depth: usize = 0;
    let mut max_seen: usize = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for ch in json.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' | '[' if !in_string => {
                depth += 1;
                max_seen = max_seen.max(depth);
                if depth > max_depth {
                    return true;
                }
            }
            '}' | ']' if !in_string => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    false
}

/// Extract a string field from tool_input.
pub fn get_string_field(input: &HookInput, field: &str) -> Option<String> {
    input
        .tool_input
        .get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extract file_path from tool_input.
pub fn get_file_path(input: &HookInput) -> Option<String> {
    get_string_field(input, "file_path")
}

/// Extract command from tool_input (for Bash tool).
pub fn get_command(input: &HookInput) -> Option<String> {
    get_string_field(input, "command")
}

/// Extract old_string from tool_input (for Edit tool).
#[allow(dead_code)]
pub fn get_old_string(input: &HookInput) -> Option<String> {
    get_string_field(input, "old_string")
}

/// Extract new_string from tool_input (for Edit tool).
#[allow(dead_code)]
pub fn get_new_string(input: &HookInput) -> Option<String> {
    get_string_field(input, "new_string")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hook_input() {
        let json = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/test.md"},"session_id":"abc123"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "Read");
        assert_eq!(get_file_path(&input), Some("/tmp/test.md".to_string()));
        assert_eq!(input.session_id, Some("abc123".to_string()));
    }

    #[test]
    fn test_hook_output_allow() {
        let output = HookOutput::allow();
        assert_eq!(output.decision, "allow");
        assert!(output.reason.is_none());
        assert!(output.render_for(HookEventKind::PreToolUse).is_none());
    }

    #[test]
    fn test_hook_output_block() {
        let output = HookOutput::block("Test reason");
        assert_eq!(output.decision, "block");
        assert_eq!(output.reason, Some("Test reason".to_string()));

        let pre_tool = output.render_for(HookEventKind::PreToolUse).unwrap();
        assert_eq!(
            pre_tool,
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": "Test reason",
                }
            })
        );

        let prompt_submit = output.render_for(HookEventKind::UserPromptSubmit).unwrap();
        assert_eq!(
            prompt_submit,
            json!({
                "decision": "block",
                "reason": "Test reason",
            })
        );
    }

    #[test]
    fn test_is_deeply_nested() {
        let shallow = r#"{"a": 1}"#;
        assert!(!is_deeply_nested(shallow, 30));

        let deep = "[".repeat(35) + &"]".repeat(35);
        assert!(is_deeply_nested(&deep, 30));
    }
}
