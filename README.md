# Humanize

Rust implementation of the Humanize workflow for iterative development with independent Codex review.

Chinese version: [README_ZH.md](./README_ZH.md)

## Project Origin

This repository is a Rust rewrite of the original Humanize project:

- Original project: <https://github.com/humania-org/humanize/tree/main>

The workflow model remains compatible, but the implementation and runtime orchestration are now handled by Rust.

For Claude Code plugin packaging, the plugin package name is `humanize-rs`.

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

## Repository Layout

- `crates/core`: shared state, filesystem, git, codex, and template logic
- `crates/cli`: the `humanize` executable
- `prompt-template/`: prompt templates used by the runtime
- `skills/`: source `SKILL.md` files
- `hooks/`: hook configuration pointing to `humanize` on `PATH`
- `commands/`: command definitions for plugin/skill runtimes
- `agents/`: supporting agent definitions
- `docs/`: installation and usage docs

## Runtime Assets

### Prompt Templates

Prompt templates live under `prompt-template/`:

- `prompt-template/block/`
- `prompt-template/claude/`
- `prompt-template/codex/`
- `prompt-template/plan/`
- `prompt-template/pr-loop/`

The runtime binary embeds the prompt templates.
The top-level `prompt-template/` directory is the source of truth for development and maintenance, and release builds vendor a copy into the CLI crate for publishing.

### Skills

Source skill definitions live under `skills/`:

- `skills/ask-codex/SKILL.md`
- `skills/humanize/SKILL.md`
- `skills/humanize-gen-plan/SKILL.md`
- `skills/humanize-rlcr/SKILL.md`

Installed skills expect the `humanize` executable to be available on `PATH`.

## Installation

The recommended model is:

1. install `humanize` on `PATH`
2. install Claude integration files if needed
3. install skills if needed

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

### 2. Install For Your Target

Claude Code:

```bash
humanize install --target claude
```

Codex:

```bash
humanize install --target codex
```

Kimi:

```bash
humanize install --target kimi
```

Everything:

```bash
humanize install --target all
```

Useful options:

```bash
# override Claude install root
humanize install --target claude --plugin-root /custom/path

# override skill install root
humanize install --target codex --skills-dir /custom/skills

# preview only
humanize install --target all --dry-run
```

Default locations:

- Claude: `%APPDATA%\\humanize-rs` on Windows, `~/Library/Application Support/humanize-rs` on macOS, `${XDG_DATA_HOME:-~/.local/share}/humanize-rs` on Linux/Unix
- Codex: `${CODEX_HOME:-~/.codex}/skills/`
- Kimi: `~/.config/agents/skills/`

What each target installs:

- `claude`: `.claude-plugin/`, `hooks/`, `commands/`, `agents/`, `docs/images/`
- `codex`: skill definitions only
- `kimi`: skill definitions only
- `all`: all of the above

`humanize install` never installs the executable itself.
It assumes `humanize` is already on `PATH`.

### Claude Marketplace Installation

Claude marketplace installation is a **two-step** process:

1. install the `humanize` binary on `PATH`
2. install the Claude plugin package

Example:

```bash
cargo install humanize-cli --bin humanize
claude plugin marketplace add ./
claude plugin install humanize-rs@humania
```

Validation:

```bash
which humanize
claude plugin list
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

Update skill definitions in `skills/`, then reinstall:

```bash
humanize install --target codex
# or
humanize install --target kimi
```

## Additional Documentation

- [docs/usage.md](./docs/usage.md)
- [docs/install-for-claude.md](./docs/install-for-claude.md)
- [docs/install-for-codex.md](./docs/install-for-codex.md)
- [docs/install-for-kimi.md](./docs/install-for-kimi.md)

## Build

```bash
cargo build
cargo test
```

## License

MIT
