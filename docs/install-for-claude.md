# Install for Claude Code

Humanize uses a two-part deployment model:

- `humanize` on `PATH`
- Claude-side plugin installed by Claude Code's native plugin manager

Codex CLI is also required as the review backend.

## Prerequisites

```bash
humanize --help
codex --version
```

## Install

### Recommended Install

```bash
humanize init --global
```

This command:

- adds the Humanize marketplace source if needed
- installs or updates `humanize-rs` in user scope
- records the CLI version used for the sync

Because the plugin name is `humanize-rs`, Claude Code exposes namespaced slash commands such as:

```bash
/humanize-rs:gen-plan --input draft.md --output docs/plan.md
/humanize-rs:start-rlcr-loop docs/plan.md
```

Validate:

```bash
humanize init --global --show
humanize doctor
```

## Uninstall

To reverse the host-side install for Claude Code:

```bash
humanize uninstall --global
```

This removes the Humanize plugin bundle from Claude Code user scope, including host-managed skills and slash commands.

### Legacy Manual Plugin Install

From GitHub:

```bash
claude plugin marketplace add https://github.com/Cupnfish/humanize-rs.git
claude plugin install humanize-rs@humania-rs
```

## Version Sync

`humanize init --global` writes a sync stamp under `~/.claude/`.
When the `humanize` CLI version changes later, CLI commands warn if the Claude plugin was last synced by a different version.
`humanize doctor` summarizes this state in one place.

Project maintainers should use:

```bash
cargo xtask sync-version
cargo xtask verify-version-sync
```

The `humanize` binary itself is not bundled into the plugin.
It must already be available on `PATH`.

## Validate

```bash
humanize --help
humanize init --global --show
humanize doctor
humanize uninstall --global
```
