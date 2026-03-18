//! Constants used throughout the Humanize plugin.

/// Environment variable for the project directory.
pub const ENV_CLAUDE_PROJECT_DIR: &str = "CLAUDE_PROJECT_DIR";

/// Environment variable to bypass Codex sandbox (dangerous).
pub const ENV_CODEX_BYPASS_SANDBOX: &str = "HUMANIZE_CODEX_BYPASS_SANDBOX";

/// Default maximum iterations for RLCR loops.
pub const DEFAULT_MAX_ITERATIONS: u32 = 42;

/// Default Codex model.
pub const DEFAULT_CODEX_MODEL: &str = "gpt-5.4";

/// Default Codex effort level.
pub const DEFAULT_CODEX_EFFORT: &str = "xhigh";

/// Default Codex timeout in seconds.
pub const DEFAULT_CODEX_TIMEOUT_SECS: u64 = 5400;

/// State file names.
pub mod state_files {
    pub const STATE_MD: &str = "state.md";
    pub const FINALIZE_STATE_MD: &str = "finalize-state.md";
    pub const COMPLETE_STATE_MD: &str = "complete-state.md";
    pub const CANCEL_STATE_MD: &str = "cancel-state.md";
    pub const STOP_STATE_MD: &str = "stop-state.md";
    pub const MAXITER_STATE_MD: &str = "maxiter-state.md";
    pub const UNEXPECTED_STATE_MD: &str = "unexpected-state.md";
    pub const APPROVE_STATE_MD: &str = "approve-state.md";
    pub const MERGED_STATE_MD: &str = "merged-state.md";
    pub const CLOSED_STATE_MD: &str = "closed-state.md";
}

/// Signal files for session handshake.
pub mod signal_files {
    pub const PENDING_SESSION_ID: &str = ".pending-session-id";
}

/// YAML frontmatter delimiters.
pub const YAML_FRONTMATTER_START: &str = "---";
pub const YAML_FRONTMATTER_END: &str = "---";

/// Maximum JSON nesting depth allowed.
pub const MAX_JSON_DEPTH: usize = 30;

/// Terminal state file names for RLCR loops.
pub const RLCR_TERMINAL_STATES: &[&str] = &[
    "complete-state.md",
    "stop-state.md",
    "maxiter-state.md",
    "unexpected-state.md",
    "cancel-state.md",
];

/// Terminal state file names for PR loops.
pub const PR_TERMINAL_STATES: &[&str] = &[
    "approve-state.md",
    "maxiter-state.md",
    "merged-state.md",
    "closed-state.md",
    "cancel-state.md",
];
