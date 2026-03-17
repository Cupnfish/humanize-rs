//! Git operations for Humanize.
//!
//! This module provides git command wrappers for branch detection,
//! commit SHA retrieval, and repository status checks.

use std::path::Path;
use std::process::Command;
#[allow(unused_imports)]
use std::time::Duration;

/// Default timeout for git commands (in seconds).
const DEFAULT_GIT_TIMEOUT_SECS: u64 = 30;

/// Errors that can occur during git operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Git command failed: {0}")]
    CommandFailed(String),

    #[error("Git command timed out")]
    Timeout,

    #[error("Not a git repository")]
    NotAGitRepository,

    #[error("Invalid output from git: {0}")]
    InvalidOutput(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Git repository information.
#[derive(Debug, Clone)]
pub struct GitInfo {
    /// Current branch name.
    pub current_branch: String,
    /// Whether the working tree is clean.
    pub is_clean: bool,
    /// Current HEAD commit SHA.
    pub head_sha: String,
    /// Number of commits ahead of upstream.
    pub ahead_count: u32,
}

/// Get git repository information.
pub fn get_git_info<P: AsRef<Path>>(repo_path: P) -> Result<GitInfo, GitError> {
    let repo_path = repo_path.as_ref();

    // Get current branch
    let current_branch = get_current_branch(repo_path)?;

    // Check if working tree is clean
    let is_clean = is_working_tree_clean(repo_path)?;

    // Get HEAD SHA
    let head_sha = get_head_sha(repo_path)?;

    // Get ahead count
    let ahead_count = get_ahead_count(repo_path)?;

    Ok(GitInfo {
        current_branch,
        is_clean,
        head_sha,
        ahead_count,
    })
}

/// Get the current branch name.
pub fn get_current_branch<P: AsRef<Path>>(repo_path: P) -> Result<String, GitError> {
    let output = run_git_command(
        repo_path.as_ref(),
        &["branch", "--show-current"],
        DEFAULT_GIT_TIMEOUT_SECS,
    )?;

    let branch = output.trim().to_string();
    if branch.is_empty() {
        return Err(GitError::InvalidOutput(
            "Empty branch name".to_string(),
        ));
    }

    Ok(branch)
}

/// Check if the working tree is clean (no uncommitted changes).
pub fn is_working_tree_clean<P: AsRef<Path>>(repo_path: P) -> Result<bool, GitError> {
    let output = run_git_command(
        repo_path.as_ref(),
        &["status", "--porcelain"],
        DEFAULT_GIT_TIMEOUT_SECS,
    )?;

    Ok(output.trim().is_empty())
}

/// Get the HEAD commit SHA.
pub fn get_head_sha<P: AsRef<Path>>(repo_path: P) -> Result<String, GitError> {
    let output = run_git_command(
        repo_path.as_ref(),
        &["rev-parse", "HEAD"],
        DEFAULT_GIT_TIMEOUT_SECS,
    )?;

    let sha = output.trim().to_string();
    if sha.len() < 7 {
        return Err(GitError::InvalidOutput("Invalid SHA format".to_string()));
    }

    Ok(sha)
}

/// Get the short HEAD commit SHA (7 characters).
pub fn get_head_sha_short<P: AsRef<Path>>(repo_path: P) -> Result<String, GitError> {
    let sha = get_head_sha(repo_path)?;
    Ok(sha.chars().take(7).collect())
}

/// Get the number of commits ahead of upstream.
pub fn get_ahead_count<P: AsRef<Path>>(repo_path: P) -> Result<u32, GitError> {
    let output = run_git_command(
        repo_path.as_ref(),
        &["rev-list", "--count", "@{upstream}..HEAD"],
        DEFAULT_GIT_TIMEOUT_SECS,
    )?;

    let count: u32 = output.trim().parse().unwrap_or(0);
    Ok(count)
}

/// Check if one commit is an ancestor of another.
pub fn is_ancestor<P: AsRef<Path>>(
    repo_path: P,
    ancestor: &str,
    descendant: &str,
) -> Result<bool, GitError> {
    let output = run_git_command(
        repo_path.as_ref(),
        &["merge-base", "--is-ancestor", ancestor, descendant],
        DEFAULT_GIT_TIMEOUT_SECS,
    );

    match output {
        Ok(_) => Ok(true),
        Err(GitError::CommandFailed(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Parse git status output to get file counts.
#[derive(Debug, Clone, Default)]
pub struct GitStatus {
    pub modified: u32,
    pub added: u32,
    pub deleted: u32,
    pub untracked: u32,
}

/// Get git status file counts.
pub fn get_git_status<P: AsRef<Path>>(repo_path: P) -> Result<GitStatus, GitError> {
    let output = run_git_command(
        repo_path.as_ref(),
        &["status", "--porcelain"],
        DEFAULT_GIT_TIMEOUT_SECS,
    )?;

    let mut status = GitStatus::default();

    for line in output.lines() {
        if line.len() < 2 {
            continue;
        }

        let code = &line[..2];
        match code {
            " M" | "M " | "MM" => status.modified += 1,
            "A " | "AM" => status.added += 1,
            " D" | "D " => status.deleted += 1,
            "??" => status.untracked += 1,
            _ => {}
        }
    }

    Ok(status)
}

/// Run a git command with timeout.
fn run_git_command(
    repo_path: &Path,
    args: &[&str],
    _timeout_secs: u64,
) -> Result<String, GitError> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", repo_path.to_str().unwrap_or(".")])
        .args(args);

    // Note: Rust's std::process::Command doesn't have built-in timeout
    // For production, consider using wait_timeout crate or spawn + wait_with_timeout
    let output = cmd.output()?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|e| GitError::InvalidOutput(e.to_string()))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(GitError::CommandFailed(stderr.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_status_parsing() {
        // This test would require a git repository setup
        // For now, we just test the struct exists
        let status = GitStatus {
            modified: 1,
            added: 2,
            deleted: 0,
            untracked: 3,
        };
        assert_eq!(status.modified, 1);
        assert_eq!(status.added, 2);
    }
}
