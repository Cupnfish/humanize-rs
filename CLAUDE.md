# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Humanize-rs is a Rust rewrite of [humanize](https://github.com/humania-org/humanize/tree/main), providing iterative development workflows with independent Codex review. The rewrite targets better Windows compatibility since bash-based hooks are unreliable on Windows.

Three main workflow families:
- **RLCR**: iterative implementation + Codex review loops
- **PR loop**: pull request review-bot tracking and validation
- **ask-codex**: one-shot Codex consultation

## Build & Test Commands

```bash
cargo build                      # Debug build
cargo build --release            # Release build
cargo test                       # Run all tests
cargo test -p humanize-cli       # CLI crate tests only
cargo test -p humanize-cli-core  # Core crate tests only
cargo test resume                # Run tests matching "resume"
cargo clippy                     # Lint
cargo fmt --check                # Format check
cargo xtask sync-version         # Sync version across Cargo.toml, plugin.json, marketplace.json
cargo xtask verify-version-sync  # Verify version consistency
```

## Architecture

### Crate Structure

- **`crates/core`** (`humanize-cli-core`): Pure library with no CLI deps. State management (`state.rs`), hook validation (`hooks.rs`), filesystem guards (`fs.rs`), git operations (`git.rs`), Codex integration (`codex.rs`), template rendering (`template.rs`).
- **`crates/cli`** (`humanize`): The binary. Subcommands under `src/commands/`: `setup`, `resume`, `cancel`, `stop`, `hook_validation`, `monitor`, `gate`, `ask_codex`, `gen_plan`, `gen_draft`, `init`, `doctor`.
- **`xtask/`**: Build automation (version sync).

### Host Plugin Assets (installed into Claude Code / Droid)

- `hooks/hooks.json` — Hook definitions (PreToolUse, PostToolUse, Stop, StopFailure validators)
- `commands/` — Slash command definitions (`.md` files)
- `skills/` — Skill definitions (`SKILL.md` files)
- `agents/` — Agent definitions (draft-relevance-checker, plan-compliance-checker)
- `prompt-template/` — Prompt templates embedded into binary at compile time

### Runtime State (`.humanize/` in target project)

Each RLCR loop creates a timestamped directory under `.humanize/rlcr/` containing:
- `state.md` / `finalize-state.md` — YAML frontmatter state (current_round, max_iterations, session_id, branches, etc.)
- `round-N-prompt.md`, `round-N-summary.md`, `round-N-review-result.md`, `round-N-review-prompt.md` — Per-round artifacts
- `.review-phase-started` — Marker file for review phase
- Terminal states: `complete-state.md`, `cancel-state.md`, `maxiter-state.md`

### Hook System

Hooks are the core enforcement mechanism. The `hooks.json` registers validators that the host (Claude Code / Droid) calls:
- **PreToolUse**: `read-validator`, `write-validator`, `edit-validator`, `bash-validator` — prevent unauthorized file access during active loops
- **PostToolUse**: `post-tool-use` — session handshake tracking
- **Stop**: `stop rlcr` / `stop pr` — main gate logic that drives loop iteration (triggers Codex review, advances rounds)
- **StopFailure**: `stop-failure` — handles API failures with recovery markers

The Stop hook is where the actual loop progression happens: it validates state, runs Codex review, creates next-round prompts, and manages phase transitions.

### Session Handshake

Resume creates `.humanize/.pending-session-id` to re-arm hooks for the new Claude session. The post-tool-use hook picks this up and binds the session to the active loop state.

## Known Bugs and Limitations

### 1. Resume does not trigger hooks (Fixed)

After the dedicated `resume` command, the Stop hook did not fire, so Codex review never triggered. The root cause was that the resume slash command used a different hook registration path than start.

**Fix**: The `resume` subcommand and slash commands have been removed. `humanize setup rlcr` now auto-detects an existing active loop and resumes it. Use `/humanize:start-rlcr-loop` for both starting new loops and resuming existing ones.

### 2. Resume phase detection errors

Phase detection in `lifecycle.rs:rlcr_resume_action()` can misidentify the current phase. Example: when a round summary is already written (should enter review/next round), resume incorrectly returns "implementation" phase. Recent commits (3ca5694) partially address this with `has_resume_content()` checks and early returns for review-pending states, but test coverage is incomplete.

### 3. Claude Code hooks freeze with large output (Critical)

**Issue**: https://github.com/anthropics/claude-code/issues/37135

Claude Code versions after 2.1.77 freeze when hooks return large output. This severely impacts the Stop hook which returns review results and next-round prompts.

**Current status**: Output compaction patches have been removed. **Requirement**: Pin to Claude Code 2.1.77 until the upstream issue is fixed.

## Development Guidelines

- Workspace version is the single source of truth in root `Cargo.toml` — use `cargo xtask sync-version` after version bumps
- Rust edition 2024, minimum rustc 1.85
- State files use YAML frontmatter format — see `crates/core/src/state.rs` for the `State` struct
- Hook validators must return JSON matching Claude Code's hook contract — see `crates/cli/src/hook_input.rs` for `HookInput`/`HookOutput` types
- Prompt templates are embedded via `include_str!` at compile time from `prompt-template/`
- Terminal state files are named `{reason}-state.md` (complete, cancel, maxiter, stop, unexpected, approve, merged, closed)
