//! State management for Humanize loops.
//!
//! This module provides parsing and serialization for the state.md files
//! used to track RLCR and PR loop progress.
//!
//! IMPORTANT: This schema must remain compatible with the historical shell-era
//! state contract so existing loop directories continue to parse correctly.

use serde::{Deserialize, Serialize, Serializer};
use std::path::{Path, PathBuf};

use crate::constants::{YAML_FRONTMATTER_END, YAML_FRONTMATTER_START};

/// Serialize Option<String> as empty string when None (not null).
/// This matches the shell behavior: `session_id:` (empty, not `session_id: null`).
fn serialize_optional_empty<S>(value: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(s) => serializer.serialize_str(s),
        None => serializer.serialize_str(""),
    }
}

fn normalize_empty_session_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || matches!(trimmed, "''" | "\"\"" | "~" | "null") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Represents the state of an RLCR or PR loop.
///
/// Schema matches setup-rlcr-loop.sh exactly:
/// All field names use snake_case as per YAML convention.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct State {
    /// Current round number (0-indexed).
    #[serde(default)]
    pub current_round: u32,

    /// Maximum number of iterations allowed.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Codex model name (e.g., "gpt-5.4").
    #[serde(default = "default_codex_model")]
    pub codex_model: String,

    /// Codex reasoning effort (e.g., "high", "xhigh").
    #[serde(default = "default_codex_effort")]
    pub codex_effort: String,

    /// Codex timeout in seconds.
    #[serde(default = "default_codex_timeout")]
    pub codex_timeout: u64,

    /// Whether to push after each round.
    #[serde(default)]
    pub push_every_round: bool,

    /// Interval for full alignment checks (round N-1 for N, 2N-1, etc.).
    #[serde(default = "default_full_review_round")]
    pub full_review_round: u32,

    /// Path to the plan file (relative to project root).
    #[serde(default)]
    pub plan_file: String,

    /// Whether the plan file is tracked in git.
    #[serde(default)]
    pub plan_tracked: bool,

    /// Branch where the loop started.
    #[serde(default)]
    pub start_branch: String,

    /// Base branch for code review.
    #[serde(default)]
    pub base_branch: String,

    /// Base commit SHA.
    #[serde(default)]
    pub base_commit: String,

    /// Whether review phase has started.
    #[serde(default)]
    pub review_started: bool,

    /// Whether to ask Codex for clarification.
    #[serde(default = "default_ask_codex_question")]
    pub ask_codex_question: bool,

    /// Session identifier for this loop.
    /// Always serialized as empty string when None (shell contract).
    #[serde(default, serialize_with = "serialize_optional_empty")]
    pub session_id: Option<String>,

    /// Whether agent teams mode is enabled.
    #[serde(default)]
    pub agent_teams: bool,

    /// Timestamp when the loop was created (ISO 8601).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,

    // PR loop specific fields
    /// PR number for PR loops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u32>,

    /// List of configured bots for PR review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_bots: Option<Vec<String>>,

    /// List of active bots for PR review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_bots: Option<Vec<String>>,

    /// Polling interval for PR state checks (seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval: Option<u64>,

    /// Timeout for PR polling (seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_timeout: Option<u64>,

    /// Startup case for PR loop (e.g., "new_pr", "existing_pr").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_case: Option<String>,

    /// Latest commit SHA for PR.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_commit_sha: Option<String>,

    /// Timestamp of latest commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_commit_at: Option<String>,

    /// Timestamp of last trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_trigger_at: Option<String>,

    /// ID of trigger comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_comment_id: Option<String>,
}

// Default functions for serde
fn default_max_iterations() -> u32 {
    42
}

fn default_codex_model() -> String {
    "gpt-5.4".to_string()
}

fn default_codex_effort() -> String {
    "xhigh".to_string()
}

fn default_codex_timeout() -> u64 {
    5400
}

fn default_full_review_round() -> u32 {
    5
}

fn default_ask_codex_question() -> bool {
    true
}

impl Default for State {
    fn default() -> Self {
        Self {
            current_round: 0,
            max_iterations: default_max_iterations(),
            codex_model: default_codex_model(),
            codex_effort: default_codex_effort(),
            codex_timeout: default_codex_timeout(),
            push_every_round: false,
            full_review_round: default_full_review_round(),
            plan_file: String::new(),
            plan_tracked: false,
            start_branch: String::new(),
            base_branch: String::new(),
            base_commit: String::new(),
            review_started: false,
            ask_codex_question: default_ask_codex_question(),
            session_id: None,
            agent_teams: false,
            started_at: None,
            // PR loop fields
            pr_number: None,
            configured_bots: None,
            active_bots: None,
            poll_interval: None,
            poll_timeout: None,
            startup_case: None,
            latest_commit_sha: None,
            latest_commit_at: None,
            last_trigger_at: None,
            trigger_comment_id: None,
        }
    }
}

impl State {
    /// Parse state from a file containing YAML frontmatter.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, StateError> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| StateError::IoError(e.to_string()))?;
        Self::from_markdown(&content)
    }

    /// Parse state from markdown content with YAML frontmatter.
    pub fn from_markdown(content: &str) -> Result<Self, StateError> {
        let content = content.trim();

        // Check for YAML frontmatter start
        if !content.starts_with(YAML_FRONTMATTER_START) {
            return Err(StateError::MissingFrontmatter);
        }

        // Find the closing delimiter (must be on its own line)
        let rest = &content[YAML_FRONTMATTER_START.len()..];
        let end_pos = rest
            .find("\n---")
            .ok_or(StateError::MissingFrontmatterEnd)?;

        let yaml_content = &rest[..end_pos];

        // Parse YAML
        let state: State = serde_yaml::from_str(yaml_content)
            .map_err(|e| StateError::YamlParseError(e.to_string()))?;

        // Ensure defaults are applied for missing fields
        // (serde's default attribute handles this for Option types and defaults)

        Ok(state)
    }

    /// Parse state from markdown with strict validation of required fields.
    ///
    /// This matches the shell behavior in loop-common.sh parse_state_file_strict()
    /// which rejects missing required fields: current_round, max_iterations,
    /// review_started, and base_branch.
    pub fn from_markdown_strict(content: &str) -> Result<Self, StateError> {
        let content = content.trim();

        // Check for YAML frontmatter start
        if !content.starts_with(YAML_FRONTMATTER_START) {
            return Err(StateError::MissingFrontmatter);
        }

        // Find the closing delimiter
        let rest = &content[YAML_FRONTMATTER_START.len()..];
        let end_pos = rest
            .find("\n---")
            .ok_or(StateError::MissingFrontmatterEnd)?;

        let yaml_content = &rest[..end_pos];

        // First parse as generic YAML to check for required fields
        let yaml_value: serde_yaml::Value = serde_yaml::from_str(yaml_content)
            .map_err(|e| StateError::YamlParseError(e.to_string()))?;

        let mapping = yaml_value.as_mapping().ok_or_else(|| {
            StateError::MissingRequiredField("YAML must be a mapping".to_string())
        })?;

        // Validate required fields per loop-common.sh parse_state_file_strict
        let required_fields = [
            "current_round",
            "max_iterations",
            "review_started",
            "base_branch",
        ];
        for field in &required_fields {
            if !mapping.contains_key(serde_yaml::Value::String(field.to_string())) {
                return Err(StateError::MissingRequiredField(field.to_string()));
            }
        }

        // Now parse into State struct
        let state: State = serde_yaml::from_str(yaml_content)
            .map_err(|e| StateError::YamlParseError(e.to_string()))?;

        Ok(state)
    }

    /// Serialize state to markdown with YAML frontmatter.
    pub fn to_markdown(&self) -> Result<String, StateError> {
        let mut yaml = serde_yaml::to_string(self)
            .map_err(|e| StateError::YamlSerializeError(e.to_string()))?;
        yaml = yaml.replace("session_id: ''\n", "session_id:\n");
        yaml = yaml.replace("session_id: \"\"\n", "session_id:\n");

        // Format: ---\n<yaml>\n---\n\n
        // This matches the Bash implementation's format
        Ok(format!(
            "{}\n{}\n{}\n\n",
            YAML_FRONTMATTER_START,
            yaml.trim_end(),
            YAML_FRONTMATTER_END
        ))
    }

    /// Save state to a file.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), StateError> {
        let content = self.to_markdown()?;
        std::fs::write(path.as_ref(), content).map_err(|e| StateError::IoError(e.to_string()))?;
        Ok(())
    }

    /// Create a new RLCR state with the given parameters.
    pub fn new_rlcr(
        plan_file: String,
        plan_tracked: bool,
        start_branch: String,
        base_branch: String,
        base_commit: String,
        max_iterations: Option<u32>,
        codex_model: Option<String>,
        codex_effort: Option<String>,
        codex_timeout: Option<u64>,
        push_every_round: bool,
        full_review_round: Option<u32>,
        ask_codex_question: bool,
        agent_teams: bool,
        review_started: bool,
    ) -> Self {
        let now = chrono_lite_now();
        Self {
            current_round: 0,
            max_iterations: max_iterations.unwrap_or_else(default_max_iterations),
            codex_model: codex_model.unwrap_or_else(default_codex_model),
            codex_effort: codex_effort.unwrap_or_else(default_codex_effort),
            codex_timeout: codex_timeout.unwrap_or_else(default_codex_timeout),
            push_every_round,
            full_review_round: full_review_round.unwrap_or_else(default_full_review_round),
            plan_file,
            plan_tracked,
            start_branch,
            base_branch,
            base_commit,
            review_started,
            ask_codex_question,
            session_id: None, // Empty initially, filled by PostToolUse hook
            agent_teams,
            started_at: Some(now),
            // PR loop fields (all None for RLCR)
            pr_number: None,
            configured_bots: None,
            active_bots: None,
            poll_interval: None,
            poll_timeout: None,
            startup_case: None,
            latest_commit_sha: None,
            latest_commit_at: None,
            last_trigger_at: None,
            trigger_comment_id: None,
        }
    }

    /// Increment the round counter.
    pub fn increment_round(&mut self) {
        self.current_round += 1;
    }

    /// Check if max iterations have been reached.
    pub fn is_max_iterations_reached(&self) -> bool {
        self.current_round >= self.max_iterations
    }

    /// Check if this is a terminal state filename.
    pub fn is_terminal_state_file(filename: &str) -> bool {
        let terminal_states = [
            "complete-state.md",
            "cancel-state.md",
            "maxiter-state.md",
            "stop-state.md",
            "unexpected-state.md",
            "approve-state.md",
            "merged-state.md",
            "closed-state.md",
        ];
        terminal_states.contains(&filename)
    }

    /// Check if a reason is a valid terminal state reason.
    pub fn is_valid_terminal_reason(reason: &str) -> bool {
        matches!(
            reason,
            "complete"
                | "cancel"
                | "maxiter"
                | "stop"
                | "unexpected"
                | "approve"
                | "merged"
                | "closed"
        )
    }

    /// Get the terminal state filename for a given exit reason.
    ///
    /// Returns None if the reason is not a valid terminal reason.
    /// Shell contract: invalid reasons should error, not silently map to unexpected.
    pub fn terminal_state_filename(reason: &str) -> Option<&'static str> {
        match reason {
            "complete" => Some("complete-state.md"),
            "cancel" => Some("cancel-state.md"),
            "maxiter" => Some("maxiter-state.md"),
            "stop" => Some("stop-state.md"),
            "unexpected" => Some("unexpected-state.md"),
            "approve" => Some("approve-state.md"),
            "merged" => Some("merged-state.md"),
            "closed" => Some("closed-state.md"),
            _ => None,
        }
    }

    /// Rename state file to terminal state file.
    ///
    /// This implements the end-loop rename behavior from loop-common.sh:
    /// After determining the exit reason, rename state.md to <reason>-state.md
    ///
    /// Returns error if reason is not valid (matching shell end_loop behavior).
    pub fn rename_to_terminal<P: AsRef<Path>>(
        state_path: P,
        reason: &str,
    ) -> Result<PathBuf, StateError> {
        let terminal_name = Self::terminal_state_filename(reason)
            .ok_or_else(|| StateError::InvalidTerminalReason(reason.to_string()))?;

        let state_path = state_path.as_ref();
        let dir = state_path
            .parent()
            .ok_or_else(|| StateError::IoError("Cannot determine parent directory".to_string()))?;

        let terminal_path = dir.join(terminal_name);

        std::fs::rename(state_path, &terminal_path)
            .map_err(|e| StateError::IoError(e.to_string()))?;

        Ok(terminal_path)
    }
}

/// Find the active RLCR loop directory.
///
/// Matches loop-common.sh find_active_loop behavior exactly:
/// - Without session filter: only check the single newest directory (zombie-loop protection)
/// - With session filter: iterate newest-to-oldest, find first matching session
/// - Empty stored session_id matches any filter (backward compatibility)
/// - Only return if still active (has active state file, not terminal)
pub fn find_active_loop(base_dir: &Path, session_id: Option<&str>) -> Option<PathBuf> {
    if !base_dir.exists() {
        return None;
    }

    let entries = match std::fs::read_dir(base_dir) {
        Ok(e) => e,
        Err(_) => return None,
    };

    let mut dirs = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.reverse();

    if session_id.is_none() {
        // No filter: only check the single newest directory (zombie-loop protection)
        if let Some(newest_dir) = dirs.first() {
            // Only return if it has an active state file
            if resolve_active_state_file(newest_dir).is_some() {
                return Some(newest_dir.clone());
            }
        }
        return None;
    }

    // Session filter: iterate newest-to-oldest
    let filter_sid = session_id.unwrap();

    for loop_dir in dirs {
        // Check if this directory has any state file (active or terminal)
        let any_state = resolve_any_state_file(&loop_dir);
        if any_state.is_none() {
            continue;
        }

        // Read session_id from the state file
        let stored_session_id = any_state
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|content| extract_session_id_from_frontmatter(&content));

        // Empty stored session_id matches any session (backward compatibility)
        let matches_session = match stored_session_id {
            None => true, // No stored session_id, matches any
            Some(ref stored) => stored == filter_sid,
        };

        if matches_session {
            // This is the newest dir for this session -- only return if active
            if resolve_active_state_file(&loop_dir).is_some() {
                return Some(loop_dir);
            }
            // The session's newest loop is terminal; do not fall through to older dirs.
            return None;
        }
    }

    None
}

fn extract_session_id_from_frontmatter(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }

    for line in lines {
        if line == "---" {
            break;
        }

        if let Some(value) = line.strip_prefix("session_id:") {
            return normalize_empty_session_id(value.trim());
        }
    }

    None
}

/// Resolve the active state file in a loop directory.
///
/// Checks finalize-state.md FIRST (loop in finalize phase), then state.md.
/// Does NOT return terminal states - only active states.
/// Matches loop-common.sh resolve_active_state_file behavior exactly.
pub fn resolve_active_state_file(loop_dir: &Path) -> Option<PathBuf> {
    // First check for finalize-state.md (active but in finalize phase)
    let finalize_file = loop_dir.join("finalize-state.md");
    if finalize_file.exists() {
        return Some(finalize_file);
    }

    // Then check for state.md (normal active state)
    let state_file = loop_dir.join("state.md");
    if state_file.exists() {
        return Some(state_file);
    }

    None
}

/// Resolve any state file (active or terminal) in a loop directory.
///
/// Prefers active states (finalize-state.md, state.md), then falls back
/// to any terminal state file (*-state.md).
/// Matches loop-common.sh resolve_any_state_file behavior exactly.
pub fn resolve_any_state_file(loop_dir: &Path) -> Option<PathBuf> {
    // Prefer active states
    if let Some(active) = resolve_active_state_file(loop_dir) {
        return Some(active);
    }

    // Fall back to terminal states (check in order of preference)
    let terminal_states = [
        "complete-state.md",
        "cancel-state.md",
        "maxiter-state.md",
        "stop-state.md",
        "unexpected-state.md",
        "approve-state.md",
        "merged-state.md",
        "closed-state.md",
    ];

    for terminal in &terminal_states {
        let path = loop_dir.join(terminal);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Check if a loop is in finalize phase.
///
/// A loop is in finalize phase if it has a finalize-state.md file.
pub fn is_finalize_phase(loop_dir: &Path) -> bool {
    loop_dir.join("finalize-state.md").exists()
}

/// Check if a loop has a pending session handshake.
///
/// Returns true if .pending-session-id signal file exists.
pub fn has_pending_session(project_root: &Path) -> bool {
    project_root.join(".humanize/.pending-session-id").exists()
}

/// Generate a timestamp in ISO 8601 format (UTC).
fn chrono_lite_now() -> String {
    use chrono::Utc;
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Errors that can occur when working with state.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("IO error: {0}")]
    IoError(String),

    #[error("Missing YAML frontmatter")]
    MissingFrontmatter,

    #[error("Missing YAML frontmatter end delimiter")]
    MissingFrontmatterEnd,

    #[error("YAML parse error: {0}")]
    YamlParseError(String),

    #[error("YAML serialize error: {0}")]
    YamlSerializeError(String),

    #[error("Missing required field: {0}")]
    MissingRequiredField(String),

    #[error("Invalid terminal reason: {0}")]
    InvalidTerminalReason(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_to_markdown() {
        let state = State::default();
        let md = state.to_markdown().unwrap();
        assert!(md.starts_with("---\n"));
        assert!(md.contains("current_round: 0"));
        assert!(md.contains("session_id:\n"));
        assert!(!md.contains("session_id: ''"));
    }

    #[test]
    fn test_state_from_markdown() {
        let content = r#"---
current_round: 1
max_iterations: 42
codex_model: gpt-5.4
codex_effort: high
codex_timeout: 5400
push_every_round: false
full_review_round: 5
plan_file: docs/plan.md
plan_tracked: false
start_branch: master
base_branch: master
base_commit: abc123
review_started: false
ask_codex_question: true
session_id:
agent_teams: false
---

Some content below.
"#;

        let state = State::from_markdown(content).unwrap();
        assert_eq!(state.current_round, 1);
        assert_eq!(state.plan_file, "docs/plan.md");
        assert!(state.session_id.is_none());
    }

    #[test]
    fn test_state_roundtrip() {
        let original = State::new_rlcr(
            "docs/plan.md".to_string(),
            false,
            "master".to_string(),
            "master".to_string(),
            "abc123".to_string(),
            None,
            None,
            None,
            None,
            false,
            None,
            true,
            false,
            false,
        );

        let md = original.to_markdown().unwrap();
        let parsed = State::from_markdown(&md).unwrap();

        assert_eq!(original.current_round, parsed.current_round);
        assert_eq!(original.plan_file, parsed.plan_file);
        assert_eq!(original.base_branch, parsed.base_branch);
    }

    #[test]
    fn test_terminal_state_filename() {
        assert_eq!(
            State::terminal_state_filename("complete"),
            Some("complete-state.md")
        );
        assert_eq!(
            State::terminal_state_filename("cancel"),
            Some("cancel-state.md")
        );
        assert_eq!(
            State::terminal_state_filename("maxiter"),
            Some("maxiter-state.md")
        );
        assert_eq!(State::terminal_state_filename("unknown"), None); // Invalid reason returns None
    }

    #[test]
    fn test_strict_parsing_rejects_missing_required_fields() {
        // Missing base_branch
        let content_missing_base = r#"---
current_round: 0
max_iterations: 42
review_started: false
---
"#;
        let result = State::from_markdown_strict(content_missing_base);
        assert!(result.is_err());
        match result {
            Err(StateError::MissingRequiredField(field)) => {
                assert_eq!(field, "base_branch");
            }
            _ => panic!("Expected MissingRequiredField error for base_branch"),
        }

        // Missing max_iterations
        let content_missing_max = r#"---
current_round: 0
review_started: false
base_branch: master
---
"#;
        let result = State::from_markdown_strict(content_missing_max);
        assert!(result.is_err());

        // Valid state with all required fields
        let content_valid = r#"---
current_round: 0
max_iterations: 42
review_started: false
base_branch: master
---
"#;
        let result = State::from_markdown_strict(content_valid);
        assert!(result.is_ok());
    }

    #[test]
    fn find_active_loop_uses_directory_name_order_without_session_filter() {
        let tempdir = tempfile::tempdir().unwrap();
        let base = tempdir.path().join("rlcr");
        std::fs::create_dir_all(&base).unwrap();

        let older = base.join("2026-03-18_00-00-00");
        let newer = base.join("2026-03-19_00-00-00");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::create_dir_all(&newer).unwrap();

        std::fs::write(
            older.join("state.md"),
            "---\nsession_id: old\ncurrent_round: 0\nmax_iterations: 1\nplan_file: plan.md\nplan_tracked: false\nstart_branch: main\nbase_branch: main\nbase_commit: abc\nreview_started: false\n---\n",
        )
        .unwrap();
        std::fs::write(
            newer.join("cancel-state.md"),
            "---\nsession_id: new\ncurrent_round: 0\nmax_iterations: 1\nplan_file: plan.md\nplan_tracked: false\nstart_branch: main\nbase_branch: main\nbase_commit: abc\nreview_started: false\n---\n",
        )
        .unwrap();

        assert_eq!(find_active_loop(&base, None), None);
    }

    #[test]
    fn find_active_loop_stops_at_newest_terminal_match_for_session() {
        let tempdir = tempfile::tempdir().unwrap();
        let base = tempdir.path().join("rlcr");
        std::fs::create_dir_all(&base).unwrap();

        let older = base.join("2026-03-18_00-00-00");
        let newer = base.join("2026-03-19_00-00-00");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::create_dir_all(&newer).unwrap();

        let state = "---\nsession_id: session-1\ncurrent_round: 0\nmax_iterations: 1\nplan_file: plan.md\nplan_tracked: false\nstart_branch: main\nbase_branch: main\nbase_commit: abc\nreview_started: false\n---\n";
        std::fs::write(older.join("state.md"), state).unwrap();
        std::fs::write(newer.join("cancel-state.md"), state).unwrap();

        assert_eq!(find_active_loop(&base, Some("session-1")), None);
    }
}
