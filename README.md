# Humanize

Rust implementation of the Humanize workflow for iterative development with independent Codex review.

Chinese version: [README_ZH.md](./README_ZH.md)

## Project Origin

This repository is a Rust rewrite of the original Humanize project:

- Original project: <https://github.com/humania-org/humanize/tree/main>

The workflow model remains compatible, but the implementation and runtime orchestration are now handled by Rust.

The shared plugin package name is `humanize-rs`.

## Overview

Humanize provides three main workflow families:

- `RLCR`: iterative implementation plus Codex review
- `PR loop`: review-bot tracking and validation for pull requests
- `ask-codex`: one-shot Codex consultation

## Workflow

RLCR workflow overview:

![RLCR Workflow](docs/images/rlcr-workflow.svg)

State is stored under `.humanize/` in the working project:

- `.humanize/rlcr/`
- `.humanize/pr-loop/`
- `.humanize/skill/`

## Architecture

Humanize is now organized as two shipped artifacts plus one external backend:

1. `humanize` binary
   The Rust runtime engine. It embeds the prompt templates and owns all loop, hook, validation, monitor, and Codex orchestration logic.
2. Plugin package
   The repository-root plugin contents: `.claude-plugin/`, `hooks/`, `commands/`, `agents/`, `skills/`, and `docs/images/`.
   This same package is used by both Claude Code and Droid.
3. Codex CLI
   The review backend used by RLCR, PR validation, and `ask-codex`.

There is no separate Codex-host or Kimi-host installation path anymore.
Codex is kept only as the independent reviewer backend.

## Repository Layout

- `crates/core`: shared state, filesystem, git, codex, and template logic
- `crates/cli`: the `humanize` executable
- `prompt-template/`: source prompt templates embedded into the binary
- `skills/`: plugin-bundled `SKILL.md` files for Claude Code and Droid
- `hooks/`: plugin hook configuration pointing to `humanize` on `PATH`
- `commands/`: plugin slash-command definitions
- `agents/`: supporting agent definitions
- `.claude-plugin/`: shared plugin metadata
- `docs/`: installation and usage docs

## Runtime Assets

### Prompt Templates

Prompt templates live under `prompt-template/`:

- `prompt-template/block/`
- `prompt-template/claude/`
- `prompt-template/codex/`
- `prompt-template/plan/`
- `prompt-template/pr-loop/`

The `humanize` binary embeds these templates.
The top-level `prompt-template/` directory is the source of truth for development and maintenance.

### Plugin Skills

Source skill definitions live under `skills/`:

- `skills/ask-codex/SKILL.md`
- `skills/humanize/SKILL.md`
- `skills/humanize-gen-plan/SKILL.md`
- `skills/humanize-rlcr/SKILL.md`

These skills are part of the plugin package for Claude Code and Droid.
They are not installed separately.

## Installation

The recommended model is:

1. install `humanize` on `PATH`
2. install `codex` on `PATH`
3. install the plugin package in Claude Code or Droid

### 1. Install `humanize` on `PATH`

From crates.io:

```bash
cargo install humanize-cli --bin humanize
```

From this repository:

```bash
cargo install --path crates/cli --bin humanize
```

Or build a release binary and place it on `PATH` manually:

```bash
cargo build --release
cp target/release/humanize /usr/local/bin/humanize
```

Verify:

```bash
which humanize
humanize --help
```

### 2. Install Codex CLI

Humanize uses Codex as an independent reviewer backend.
Install Codex CLI separately and make sure `codex` is on `PATH`.

Verify:

```bash
codex --version
```

### 3. Install the Plugin Package

The same plugin package works in both Claude Code and Droid.
Droid documents direct compatibility with Claude Code plugins, and this repository has been verified locally with `droid plugin install`.

Claude Code:

```bash
claude plugin marketplace add https://github.com/Cupnfish/humanize-rs.git
claude plugin install humanize-rs@humania-rs
```

Droid:

```bash
droid plugin marketplace add https://github.com/Cupnfish/humanize-rs.git
droid plugin install humanize-rs@humanize-rs
```

The plugin package contains:

- `.claude-plugin/`
- `hooks/`
- `commands/`
- `agents/`
- `skills/`

The `humanize` executable still comes from `PATH`.
The binary does not install plugin assets and does not install skills separately.

For local plugin development, you can still add the current checkout as a marketplace:

```bash
claude plugin marketplace add ./
claude plugin install humanize-rs@humania-rs
```

## Local Development

Inspect the CLI:

```bash
humanize --help
humanize setup rlcr --help
humanize setup pr --help
humanize monitor rlcr --help
```

If `humanize` is not installed on `PATH` yet, you can temporarily replace these examples with `cargo run -- ...` while developing locally.

## Using the Plugin

Once the plugin is installed in Claude Code or Droid, the primary user interface is the host REPL, not the raw CLI.
The plugin commands and skills call `humanize` behind the scenes.

Command naming differs by host:

- Claude Code: plugin commands and user-invocable skills are namespaced with `humanize-rs:`
- Droid: the same commands are exposed without the `humanize-rs:` prefix

### Quick Start

Claude Code:

```bash
/humanize-rs:gen-plan --input draft.md --output docs/plan.md
/humanize-rs:start-rlcr-loop docs/plan.md
/humanize-rs:resume-rlcr-loop
/humanize-rs:start-pr-loop --claude
/humanize-rs:resume-pr-loop
/humanize-rs:cancel-rlcr-loop
/humanize-rs:ask-codex Explain the latest review result
```

Droid:

```bash
/gen-plan --input draft.md --output docs/plan.md
/start-rlcr-loop docs/plan.md
/resume-rlcr-loop
/start-pr-loop --claude
/resume-pr-loop
/cancel-rlcr-loop
/ask-codex Explain the latest review result
```

Both hosts expose the same workflow families:

- generate a plan from a draft
- start an RLCR loop
- resume an existing RLCR loop from `.humanize/`
- start a PR loop
- resume an existing PR loop from `.humanize/`
- cancel an active RLCR or PR loop
- consult Codex directly

### What The Plugin Does For You

- Hooks call the native Rust validators and stop hooks automatically.
- RLCR and PR loop state is persisted under `.humanize/`.
- Bundled skills such as `humanize`, `humanize-rlcr`, `humanize-gen-plan`, and `ask-codex` are available to the host and may be auto-invoked when relevant.

### RLCR User Flow

1. In Claude Code, run `/humanize-rs:gen-plan --input draft.md --output docs/plan.md`. In Droid, run `/gen-plan --input draft.md --output docs/plan.md`.
2. In Claude Code, run `/humanize-rs:start-rlcr-loop docs/plan.md`. In Droid, run `/start-rlcr-loop docs/plan.md`.
3. Continue working normally in Claude Code or Droid.
4. When the host stops, Humanize hooks automatically validate state, run Codex review, and decide whether to continue, block, or advance the phase.
5. Use the monitor from a terminal if you want a live view of the loop state.

If the host session is lost but `.humanize/rlcr/` still exists, resume the loop instead of starting over:

- Claude Code: `/humanize-rs:resume-rlcr-loop`
- Droid: `/resume-rlcr-loop`

### Direct CLI Usage

Direct CLI usage is mainly for:

- monitor dashboards
- debugging
- manual recovery
- non-hook environments

Examples:

```bash
humanize gen-plan --input draft.md --output docs/plan.md
humanize setup rlcr docs/plan.md
humanize resume rlcr
humanize gate rlcr
humanize resume pr
humanize ask-codex "Explain the latest review result"
```

### Monitor

One-shot snapshot:

```bash
humanize monitor rlcr --once
humanize monitor pr --once
humanize monitor skill --once
```

Interactive TUI:

```bash
humanize monitor rlcr
humanize monitor pr
humanize monitor skill
```

TUI controls:

- `q` / `Esc`: quit
- `j` / `k` or arrow keys: scroll
- `PgUp` / `PgDn`: page scroll
- `g` / `G`: top / bottom
- `f`: toggle follow mode
- `r`: refresh immediately

Example RLCR monitor TUI:

![Humanize Monitor TUI](docs/images/monitor-tui.svg)

### Manual Recovery / Hook Debugging

Direct stop invocation:

```bash
printf '{}' | humanize stop rlcr
printf '{}' | humanize stop pr
```

Skill-mode or non-hook gate:

```bash
humanize gate rlcr
```

Gate exit codes:

- `0`: allowed
- `10`: blocked
- `20`: runtime / infrastructure error

## Manual Hook Testing

Example: read validator

```bash
printf '%s\n' '{"tool_name":"Read","tool_input":{"file_path":"src/main.rs"}}' \
  | humanize hook read-validator
```

Example: bash validator

```bash
printf '%s\n' '{"tool_name":"Bash","tool_input":{"command":"git add -A"}}' \
  | humanize hook bash-validator
```

## Prompt / Skill Maintenance

Update prompt templates in `prompt-template/`.

Examples:

- `prompt-template/claude/next-round-prompt.md`
- `prompt-template/codex/full-alignment-review.md`
- `prompt-template/pr-loop/round-0-task-has-comments.md`

Update plugin-facing assets in:

- `skills/`
- `hooks/`
- `commands/`
- `agents/`
- `.claude-plugin/`

Then reload or reinstall the plugin in Claude Code or Droid.

## Additional Documentation

- [docs/usage.md](./docs/usage.md)
- [docs/install-for-claude.md](./docs/install-for-claude.md)
- [docs/install-for-droid.md](./docs/install-for-droid.md)
- [docs/install-for-codex.md](./docs/install-for-codex.md)

## Build

```bash
cargo build
cargo test
```

## License

MIT
