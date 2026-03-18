# Install for Claude Code

Humanize uses a two-part deployment model:

- `humanize` on `PATH`
- the shared plugin package from this repository

Codex CLI is also required as the review backend.

## Prerequisites

```bash
humanize --help
codex --version
```

## Install

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

The plugin package includes:

- `.claude-plugin/`
- `hooks/`
- `commands/`
- `agents/`
- `skills/`

The `humanize` binary itself is not bundled into the plugin package.
It must already be available on `PATH`.

## Validate

```bash
humanize --help
claude plugin list
```
