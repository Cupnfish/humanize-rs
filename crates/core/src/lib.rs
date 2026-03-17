//! Humanize Core Library
//!
//! This library provides the core functionality for the Humanize Claude Code plugin,
//! including state management, file operations, git interactions, and hook validation.

pub mod state;
pub mod fs;
pub mod git;
pub mod hooks;
pub mod constants;

pub use state::State;
pub use constants::*;
