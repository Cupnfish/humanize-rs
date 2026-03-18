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
claude plugin marketplace add ./
claude plugin install humanize-rs@humania
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

## Local Development

Inspect the CLI:

```bash
humanize --help
humanize setup rlcr --help
humanize setup pr --help
humanize monitor rlcr --help
```

If `humanize` is not installed on `PATH` yet, you can temporarily replace these examples with `cargo run -- ...` while developing locally.

## Common Workflows

### Generate a Plan

```bash
humanize gen-plan --input draft.md --output docs/plan.md
```

The native `gen-plan` flow:

- validates input and output
- checks repository relevance
- analyzes draft ambiguity and quantitative metrics
- prompts for clarification in interactive terminals when needed
- generates the final plan
- preserves the original draft section

### Start RLCR

```bash
humanize setup rlcr docs/plan.md
```

Useful variants:

```bash
humanize setup rlcr --skip-impl
humanize setup rlcr docs/plan.md --max-iterations 20 --full-review-round 3
humanize setup rlcr docs/plan.md --push-every-round
```

### RLCR Stop / Gate

Direct stop invocation:

```bash
printf '{}' | humanize stop rlcr
```

Skill-mode or non-hook gate:

```bash
humanize gate rlcr
```

Gate exit codes:

- `0`: allowed
- `10`: blocked
- `20`: runtime / infrastructure error

### Start PR Loop

```bash
humanize setup pr --claude
humanize setup pr --codex
humanize setup pr --claude --codex
```

### Stop PR Loop

```bash
printf '{}' | humanize stop pr
```

### Cancel Loops

```bash
humanize cancel rlcr
humanize cancel pr
```

### Ask Codex

```bash
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
