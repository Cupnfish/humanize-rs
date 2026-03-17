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

/// Round file types that have special handling in validators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundFileType {
    /// Summary file (round-N-summary.md)
    Summary,
    /// Prompt file (round-N-prompt.md)
    Prompt,
    /// Todos file (round-N-todos.md)
    Todos,
}

/// Check if a path is a specific round file type.
///
/// Matches the pattern: round-N-<type>.md where N is a number.
pub fn is_round_file_type(path: &str, file_type: RoundFileType) -> bool {
    let path_lower = path.to_lowercase();
    let type_str = match file_type {
        RoundFileType::Summary => "summary",
        RoundFileType::Prompt => "prompt",
        RoundFileType::Todos => "todos",
    };

    // Extract filename
    let filename = path_lower
        .rsplit('/')
        .next()
        .unwrap_or(&path_lower);

    // Must start with "round-"
    if !filename.starts_with("round-") {
        return false;
    }

    // Must end with -<type>.md
    let suffix = format!("-{}.md", type_str);
    if !filename.ends_with(&suffix) {
        return false;
    }

    // Extract the part between "round-" and "-<type>.md"
    let rest = &filename[6..]; // skip "round-"
    if let Some(num_part) = rest.strip_suffix(&suffix) {
        // Must be all digits
        return num_part.chars().all(|c| c.is_ascii_digit()) && !num_part.is_empty();
    }

    false
}

/// Check if a path is any round file (summary, prompt, or todos).
pub fn is_any_round_file(path: &str) -> bool {
    is_round_file_type(path, RoundFileType::Summary)
        || is_round_file_type(path, RoundFileType::Prompt)
        || is_round_file_type(path, RoundFileType::Todos)
}

/// Check if a path is inside .humanize/rlcr/ or .humanize/pr-loop/.
pub fn is_in_humanize_loop_dir(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    path_lower.contains(".humanize/rlcr/") || path_lower.contains(".humanize/pr-loop/")
}

/// Extract round number from a round filename.
///
/// Returns None if the filename doesn't match round-N-*.md pattern.
pub fn extract_round_number(filename: &str) -> Option<u32> {
    let filename_lower = filename.to_lowercase();
    let filename_only = filename_lower.rsplit('/').next().unwrap_or(&filename_lower);

    if !filename_only.starts_with("round-") || !filename_only.ends_with(".md") {
        return None;
    }

    // Extract "N" from "round-N-*.md"
    let rest = &filename_only[6..filename_only.len() - 3]; // skip "round-" and ".md"

    // Find the first dash after the number
    let num_end = rest.find('-')?;
    let num_str = &rest[..num_end];

    num_str.parse().ok()
}

/// Check if a file path is allowlisted for reading during a loop.
///
/// A file is allowlisted if:
/// - It's the current round's summary/prompt file
/// - It's a historical summary file (for context)
/// - It's in the active loop directory and matches current round
pub fn is_allowlisted_file(file_path: &str, loop_dir: &std::path::Path, current_round: u32) -> bool {
    let file_path_lower = file_path.to_lowercase();
    let loop_dir_str = loop_dir.to_string_lossy().to_lowercase();

    // Must be in the loop directory
    if !file_path_lower.starts_with(&loop_dir_str) {
        return false;
    }

    // Extract filename
    let filename = file_path_lower.rsplit('/').next().unwrap_or(&file_path_lower);

    // Allow current round's summary and prompt
    let current_summary = format!("round-{}-summary.md", current_round);
    let current_prompt = format!("round-{}-prompt.md", current_round);

    if filename == current_summary || filename == current_prompt {
        return true;
    }

    // Allow historical summaries (round-N-summary.md where N < current_round)
    if is_round_file_type(file_path, RoundFileType::Summary) {
        if let Some(round) = extract_round_number(file_path) {
            if round < current_round {
                return true;
            }
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

    #[test]
    fn test_is_round_file_type() {
        // Summary files
        assert!(is_round_file_type("round-1-summary.md", RoundFileType::Summary));
        assert!(is_round_file_type(".humanize/rlcr/test/round-2-summary.md", RoundFileType::Summary));
        assert!(!is_round_file_type("round-1-prompt.md", RoundFileType::Summary));

        // Prompt files
        assert!(is_round_file_type("round-1-prompt.md", RoundFileType::Prompt));
        assert!(!is_round_file_type("round-1-summary.md", RoundFileType::Prompt));

        // Todos files
        assert!(is_round_file_type("round-1-todos.md", RoundFileType::Todos));
        assert!(!is_round_file_type("round-1-summary.md", RoundFileType::Todos));

        // Invalid patterns
        assert!(!is_round_file_type("roundup.md", RoundFileType::Summary));
        assert!(!is_round_file_type("round--summary.md", RoundFileType::Summary));
    }

    #[test]
    fn test_is_in_humanize_loop_dir() {
        assert!(is_in_humanize_loop_dir(".humanize/rlcr/2026-03-17/state.md"));
        assert!(is_in_humanize_loop_dir(".humanize/pr-loop/2026-03-17/state.md"));
        assert!(!is_in_humanize_loop_dir("src/main.rs"));
        assert!(!is_in_humanize_loop_dir(".humanize/config.json"));
    }

    #[test]
    fn test_extract_round_number() {
        assert_eq!(extract_round_number("round-1-summary.md"), Some(1));
        assert_eq!(extract_round_number("round-42-prompt.md"), Some(42));
        assert_eq!(extract_round_number(".humanize/rlcr/test/round-3-todos.md"), Some(3));
        assert_eq!(extract_round_number("roundup.md"), None);
        assert_eq!(extract_round_number("round--summary.md"), None);
        assert_eq!(extract_round_number("src/main.rs"), None);
    }
}
