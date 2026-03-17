//! State management for Humanize loops.
//!
//! This module provides parsing and serialization for the state.md files
//! used to track RLCR and PR loop progress.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::constants::{DEFAULT_MAX_ITERATIONS, YAML_FRONTMATTER_START, YAML_FRONTMATTER_END};

/// Represents the state of an RLCR or PR loop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct State {
    /// Current round number (0-indexed).
    pub current_round: u32,

    /// Maximum number of iterations allowed.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Path to the plan file (relative to project root).
    pub plan_file: String,

    /// Unique session identifier for this loop instance.
    #[serde(default)]
    pub session_id: String,

    /// Base branch for the loop (e.g., "main", "master").
    pub base_branch: String,

    /// Start branch where the loop began.
    pub start_branch: String,

    /// Base commit SHA at loop start.
    pub base_commit_sha: String,

    /// Whether agent teams mode is enabled.
    #[serde(default)]
    pub agent_teams: bool,

    /// Codex model to use.
    #[serde(default = "default_codex_model")]
    pub codex_model: String,

    /// Codex effort level.
    #[serde(default = "default_codex_effort")]
    pub codex_effort: String,

    /// Codex timeout in seconds.
    #[serde(default = "default_codex_timeout")]
    pub codex_timeout_secs: u64,

    /// Loop directory path (relative to .humanize/).
    pub loop_dir: String,

    /// Timestamp when the loop was created.
    pub created_at: String,

    /// For PR loops: the PR URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,

    /// For PR loops: list of bots being tracked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_bots: Option<Vec<String>>,

    /// For PR loops: list of bots pending approval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_bots: Option<Vec<String>>,
}

fn default_max_iterations() -> u32 {
    DEFAULT_MAX_ITERATIONS
}

fn default_codex_model() -> String {
    crate::constants::DEFAULT_CODEX_MODEL.to_string()
}

fn default_codex_effort() -> String {
    crate::constants::DEFAULT_CODEX_EFFORT.to_string()
}

fn default_codex_timeout() -> u64 {
    crate::constants::DEFAULT_CODEX_TIMEOUT_SECS
}

impl State {
    /// Create a new state with default values.
    pub fn new(
        plan_file: String,
        base_branch: String,
        start_branch: String,
        base_commit_sha: String,
        loop_dir: String,
    ) -> Self {
        let now = current_timestamp();
        Self {
            current_round: 0,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            plan_file,
            session_id: String::new(),
            base_branch,
            start_branch,
            base_commit_sha,
            agent_teams: false,
            codex_model: default_codex_model(),
            codex_effort: default_codex_effort(),
            codex_timeout_secs: default_codex_timeout(),
            loop_dir,
            created_at: now,
            pr_url: None,
            active_bots: None,
            pending_bots: None,
        }
    }

    /// Parse state from a file containing YAML frontmatter.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, StateError> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| StateError::IoError(e.to_string()))?;
        Self::from_markdown(&content)
    }

    /// Parse state from markdown content with YAML frontmatter.
    pub fn from_markdown(content: &str) -> Result<Self, StateError> {
        let content = content.trim();

        // Check for YAML frontmatter
        if !content.starts_with(YAML_FRONTMATTER_START) {
            return Err(StateError::MissingFrontmatter);
        }

        // Find the closing delimiter
        let rest = &content[YAML_FRONTMATTER_START.len()..];
        let end_pos = rest
            .find(YAML_FRONTMATTER_END)
            .ok_or(StateError::MissingFrontmatterEnd)?;

        let yaml_content = &rest[..end_pos];

        // Parse YAML
        let state: State = serde_yaml::from_str(yaml_content)
            .map_err(|e| StateError::YamlParseError(e.to_string()))?;

        Ok(state)
    }

    /// Serialize state to markdown with YAML frontmatter.
    pub fn to_markdown(&self) -> Result<String, StateError> {
        let yaml = serde_yaml::to_string(self)
            .map_err(|e| StateError::YamlSerializeError(e.to_string()))?;

        Ok(format!(
            "{}\n{}{}\n\n",
            YAML_FRONTMATTER_START,
            yaml,
            YAML_FRONTMATTER_END
        ))
    }

    /// Save state to a file.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), StateError> {
        let content = self.to_markdown()?;
        std::fs::write(path.as_ref(), content)
            .map_err(|e| StateError::IoError(e.to_string()))?;
        Ok(())
    }

    /// Check if this is a terminal state (loop has ended).
    pub fn is_terminal_state(&self) -> bool {
        // This would be determined by checking if the state file has been renamed
        // to a terminal state name. For now, return false.
        false
    }

    /// Increment the round counter.
    pub fn increment_round(&mut self) {
        self.current_round += 1;
    }

    /// Check if max iterations have been reached.
    pub fn is_max_iterations_reached(&self) -> bool {
        self.current_round >= self.max_iterations
    }
}

/// Generate a timestamp string in YYYY-MM-DD_HH-MM-SS format.
fn current_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    // Format: YYYY-MM-DD_HH-MM-SS (approximate)
    let days = secs / 86400;
    let years = 1970 + days / 365;
    let remaining_days = days % 365;
    let months = remaining_days / 30 + 1;
    let day = remaining_days % 30 + 1;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        years, months, day, hours, minutes, seconds
    )
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_to_markdown() {
        let state = State::new(
            "docs/plan.md".to_string(),
            "master".to_string(),
            "master".to_string(),
            "abc123".to_string(),
            ".humanize/rlcr/test".to_string(),
        );

        let md = state.to_markdown().unwrap();
        assert!(md.starts_with("---\n"));
        assert!(md.contains("current_round: 0"));
        assert!(md.contains("plan_file: docs/plan.md"));
    }

    #[test]
    fn test_state_from_markdown() {
        let content = r#"---
current_round: 1
max_iterations: 42
plan_file: docs/plan.md
session_id: test-session
base_branch: master
start_branch: master
base_commit_sha: abc123
loop_dir: .humanize/rlcr/test
created_at: "2026-03-17_14-00-00"
---

Some content below.
"#;

        let state = State::from_markdown(content).unwrap();
        assert_eq!(state.current_round, 1);
        assert_eq!(state.plan_file, "docs/plan.md");
        assert_eq!(state.session_id, "test-session");
    }

    #[test]
    fn test_state_roundtrip() {
        let original = State::new(
            "docs/plan.md".to_string(),
            "main".to_string(),
            "feature".to_string(),
            "def456".to_string(),
            ".humanize/rlcr/roundtrip-test".to_string(),
        );

        let md = original.to_markdown().unwrap();
        let parsed = State::from_markdown(&md).unwrap();

        assert_eq!(original.current_round, parsed.current_round);
        assert_eq!(original.plan_file, parsed.plan_file);
        assert_eq!(original.base_branch, parsed.base_branch);
    }
}
