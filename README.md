# Humanize

Rust implementation of the Humanize workflow for iterative development with independent Codex review.

Chinese version: [README_ZH.md](./README_ZH.md)

## Project Origin

This repository is a Rust rewrite of the original Humanize project:

- Original project: <https://github.com/humania-org/humanize/tree/main>

The workflow model remains compatible, but the implementation and runtime orchestration are now handled by Rust.

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
- `shims/`: optional compatibility shims that call `humanize` from `PATH`

## Runtime Assets

### Prompt Templates

Prompt templates live under `prompt-template/`:

- `prompt-template/block/`
- `prompt-template/claude/`
- `prompt-template/codex/`
- `prompt-template/plan/`
- `prompt-template/pr-loop/`

At runtime, the binary resolves templates from:

```bash
CLAUDE_PLUGIN_ROOT/prompt-template
```

For local development in this repository:

```bash
export CLAUDE_PLUGIN_ROOT="$PWD"
```

### Skills

Source skill definitions live under `skills/`:

- `skills/ask-codex/SKILL.md`
- `skills/humanize/SKILL.md`
- `skills/humanize-gen-plan/SKILL.md`
- `skills/humanize-rlcr/SKILL.md`

Installed skills use the runtime root for assets, but they expect the `humanize` executable to be available on `PATH`.

## Installation

The recommended model is:

1. install `humanize` on `PATH`
2. install runtime assets
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

### 2. Install Runtime Assets

Install the runtime assets into a plugin root:

```bash
cargo run -- install --plugin-root "$PWD"
```

This syncs:

- `prompt-template/`
- `hooks/`
- `commands/`
- `agents/`
- `skills/`
- `.claude-plugin/`

It does **not** copy the executable. The executable must already be available on `PATH`.

### 3. Install Skills

For Codex:

```bash
cargo run -- install-skills --target codex
```

For Kimi:

```bash
cargo run -- install-skills --target kimi
```

Useful variants:

```bash
# both targets
cargo run -- install-skills --target both

# custom target directory
cargo run -- install-skills --target codex --skills-dir /tmp/my-skills

# preview only
cargo run -- install-skills --target codex --dry-run
```

Default skill install locations:

- Codex: `${CODEX_HOME:-~/.codex}/skills/`
- Kimi: `~/.config/agents/skills/`

Installed skills assume `humanize` is on `PATH`.

## Local Development

From the repository root:

```bash
export CLAUDE_PLUGIN_ROOT="$PWD"
export CLAUDE_PROJECT_DIR="$PWD"
```

Inspect the CLI:

```bash
cargo run -- --help
cargo run -- setup rlcr --help
cargo run -- setup pr --help
cargo run -- monitor rlcr --help
```

## Common Workflows

### Generate a Plan

```bash
cargo run -- gen-plan --input draft.md --output docs/plan.md
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
cargo run -- setup rlcr docs/plan.md
```

Useful variants:

```bash
cargo run -- setup rlcr --skip-impl
cargo run -- setup rlcr docs/plan.md --max-iterations 20 --full-review-round 3
cargo run -- setup rlcr docs/plan.md --push-every-round
```

### RLCR Stop / Gate

Direct stop invocation:

```bash
printf '{}' | cargo run -- stop rlcr
```

Skill-mode or non-hook gate:

```bash
cargo run -- gate rlcr
```

Gate exit codes:

- `0`: allowed
- `10`: blocked
- `20`: runtime / infrastructure error

### Start PR Loop

```bash
cargo run -- setup pr --claude
cargo run -- setup pr --codex
cargo run -- setup pr --claude --codex
```

### Stop PR Loop

```bash
printf '{}' | cargo run -- stop pr
```

### Cancel Loops

```bash
cargo run -- cancel rlcr
cargo run -- cancel pr
```

### Ask Codex

```bash
cargo run -- ask-codex "Explain the latest review result"
```

### Monitor

One-shot snapshot:

```bash
cargo run -- monitor rlcr --once
cargo run -- monitor pr --once
cargo run -- monitor skill --once
```

Interactive TUI:

```bash
cargo run -- monitor rlcr
cargo run -- monitor pr
cargo run -- monitor skill
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
  | cargo run -- hook read-validator
```

Example: bash validator

```bash
printf '%s\n' '{"tool_name":"Bash","tool_input":{"command":"git add -A"}}' \
  | cargo run -- hook bash-validator
```

## Prompt / Skill Maintenance

Update prompt templates in `prompt-template/`.

Examples:

- `prompt-template/claude/next-round-prompt.md`
- `prompt-template/codex/full-alignment-review.md`
- `prompt-template/pr-loop/round-0-task-has-comments.md`

Update skill definitions in `skills/`, then reinstall:

```bash
cargo run -- install-skills --target codex
# or
cargo run -- install-skills --target kimi
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
