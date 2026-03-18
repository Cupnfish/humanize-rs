# Install for Claude Code

Humanize uses a two-part deployment model:

- `humanize` on `PATH`
- Claude-side assets installed into `~/.claude`

Codex CLI is also required as the review backend.

## Prerequisites

```bash
humanize --help
codex --version
```

## Install

### Option A: Native Claude Install

Recommended when you want a direct `rtk init -g` style setup without plugin management.

```bash
humanize init --global
# or:
humanize init --global --auto-patch
```

This installs into `~/.claude/`:

- `commands/` as `/humanize-*` slash commands
- `agents/`
- `skills/`
- Humanize hook registrations in `~/.claude/settings.json`

Validate:

```bash
humanize init --global --show
```

### Option B: Legacy Plugin Install

From GitHub:

```bash
claude plugin marketplace add https://github.com/Cupnfish/humanize-rs.git
claude plugin install humanize-rs@humania-rs
```

From a local clone for development:

```bash
claude plugin marketplace add ./
claude plugin install humanize-rs@humania-rs
```

## What Gets Installed

The native install path writes directly into `~/.claude/`.
Legacy plugin installation remains available only for compatibility.

The `humanize` binary itself is not bundled into host assets.
It must already be available on `PATH`.

## Validate

```bash
humanize --help
humanize init --global --show
```
