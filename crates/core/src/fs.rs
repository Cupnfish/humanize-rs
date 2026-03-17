//! File system operations for Humanize.
//!
//! This module provides safe file operations with path validation
//! to prevent security issues like path traversal and symlink attacks.

use std::path::{Component, PathBuf};

use crate::constants::MAX_JSON_DEPTH;

/// Errors that can occur during file operations.
#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("Path is not relative: {0}")]
    NotRelative(String),

    #[error("Path contains parent directory traversal: {0}")]
    PathTraversal(String),

    #[error("Path is absolute: {0}")]
    AbsolutePath(String),

    #[error("Symlink not allowed: {0}")]
    SymlinkNotAllowed(String),

    #[error("Path points outside repository: {0}")]
    OutsideRepository(String),

    #[error("Invalid UTF-8 in path")]
    InvalidUtf8,

    #[error("Null byte in path")]
    NullByte,

    #[error("IO error: {0}")]
    IoError(String),
}

impl From<std::io::Error> for FsError {
    fn from(e: std::io::Error) -> Self {
        FsError::IoError(e.to_string())
    }
}

/// Options for path validation.
#[derive(Debug, Clone)]
pub struct PathValidationOptions {
    /// Whether symlinks are allowed.
    pub allow_symlinks: bool,
    /// Whether absolute paths are allowed.
    pub allow_absolute: bool,
    /// Whether parent directory traversal (..) is allowed.
    pub allow_parent_traversal: bool,
    /// Repository root for boundary checking.
    pub repo_root: Option<PathBuf>,
}

impl Default for PathValidationOptions {
    fn default() -> Self {
        Self {
            allow_symlinks: false,
            allow_absolute: false,
            allow_parent_traversal: false,
            repo_root: None,
        }
    }
}

/// Validate a path for security.
pub fn validate_path(path: &str, options: &PathValidationOptions) -> Result<PathBuf, FsError> {
    // Check for null bytes
    if path.contains('\0') {
        return Err(FsError::NullByte);
    }

    // Check for valid UTF-8 (already guaranteed by &str)

    let parsed = PathBuf::from(path);

    // Check for absolute paths
    if parsed.is_absolute() && !options.allow_absolute {
        return Err(FsError::AbsolutePath(path.to_string()));
    }

    // Check for parent directory traversal
    if !options.allow_parent_traversal {
        for component in parsed.components() {
            if matches!(component, Component::ParentDir) {
                return Err(FsError::PathTraversal(path.to_string()));
            }
        }
    }

    // Check symlinks if we have a repo root
    if let Some(ref repo_root) = options.repo_root {
        let full_path = repo_root.join(&parsed);

        // Check if path resolves outside repo (symlink check)
        if full_path.exists() {
            if let Ok(canonical) = full_path.canonicalize() {
                if !canonical.starts_with(repo_root) {
                    return Err(FsError::OutsideRepository(path.to_string()));
                }
            }
        }
    }

    Ok(parsed)
}

/// Validate JSON depth to prevent DoS attacks.
pub fn validate_json_depth(json: &str) -> Result<(), FsError> {
    let mut depth: usize = 0;
    let mut max_depth: usize = 0;
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
                max_depth = max_depth.max(depth);
                if depth > MAX_JSON_DEPTH {
                    return Err(FsError::IoError(format!(
                        "JSON nesting depth exceeds maximum ({})",
                        MAX_JSON_DEPTH
                    )));
                }
            }
            '}' | ']' if !in_string => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    Ok(())
}

/// Check if a path is a round-specific file that should be blocked from reading.
pub fn is_round_specific_file(path: &str) -> bool {
    let path_lower = path.to_lowercase();

    // Match round-N-summary.md, round-N-review-*.md, etc.
    if path_lower.contains("/round-") || path_lower.starts_with("round-") {
        // Check if it matches round-N-*.md pattern
        let parts: Vec<&str> = path_lower.split('/').collect();
        if let Some(filename) = parts.last() {
            if filename.starts_with("round-") && filename.ends_with(".md") {
                // Check if it has a number after "round-"
                let rest = &filename[6..]; // skip "round-"
                if rest.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                    return true;
                }
            }
        }
    }

    false
}

/// Check if a path is a protected state file that should be blocked from writing.
pub fn is_protected_state_file(path: &str) -> bool {
    let path_lower = path.to_lowercase();

    // Check for state.md in .humanize/rlcr/*/ or .humanize/pr-loop/*/
    if path_lower.contains(".humanize/rlcr/") || path_lower.contains(".humanize/pr-loop/") {
        if path_lower.ends_with("/state.md") {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path_relative() {
        let options = PathValidationOptions::default();
        assert!(validate_path("src/main.rs", &options).is_ok());
    }

    #[test]
    fn test_validate_path_absolute_rejected() {
        let options = PathValidationOptions::default();
        assert!(matches!(
            validate_path("/etc/passwd", &options),
            Err(FsError::AbsolutePath(_))
        ));
    }

    #[test]
    fn test_validate_path_traversal_rejected() {
        let options = PathValidationOptions::default();
        assert!(matches!(
            validate_path("../../../etc/passwd", &options),
            Err(FsError::PathTraversal(_))
        ));
    }

    #[test]
    fn test_is_round_specific_file() {
        assert!(is_round_specific_file("round-1-summary.md"));
        assert!(is_round_specific_file(
            ".humanize/rlcr/test/round-2-review-prompt.md"
        ));
        assert!(!is_round_specific_file("src/main.rs"));
        assert!(!is_round_specific_file("roundup.md"));
    }

    #[test]
    fn test_is_protected_state_file() {
        assert!(is_protected_state_file(
            ".humanize/rlcr/2026-03-17/state.md"
        ));
        assert!(is_protected_state_file(
            ".humanize/pr-loop/2026-03-17/state.md"
        ));
        assert!(!is_protected_state_file("docs/state.md"));
        assert!(!is_protected_state_file(
            ".humanize/rlcr/2026-03-17/complete-state.md"
        ));
    }

    #[test]
    fn test_json_depth_validation() {
        assert!(validate_json_depth(r#"{"a": 1}"#).is_ok());
        assert!(validate_json_depth(r#"[[[[[[]]]]]]"#).is_ok());

        // Create deeply nested JSON
        let deep_json = "[".repeat(35) + &"]".repeat(35);
        assert!(validate_json_depth(&deep_json).is_err());
    }
}
