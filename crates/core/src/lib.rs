//! Humanize Core Library
//!
//! This library provides the core functionality for the Humanize plugin workflows,
//! including state management, file operations, git interactions, and hook validation.

pub mod codex;
pub mod constants;
pub mod fs;
pub mod git;
pub mod hooks;
pub mod state;
pub mod template;

pub use constants::*;
pub use state::State;
