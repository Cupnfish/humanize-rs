# Install for Droid

Humanize uses the same plugin package for both Claude Code and Droid.
Droid documents compatibility with Claude Code plugins, and this repository has been validated locally with `droid plugin install`.
No separate Droid-only asset bundle is maintained.

## Prerequisites

```bash
humanize --help
codex --version
droid --version
```

## Install

From GitHub:

```bash
droid plugin marketplace add https://github.com/Cupnfish/humanize-rs.git
droid plugin install humanize-rs@humanize-rs
```

From a local clone:

```bash
droid plugin marketplace add /path/to/humanize-rs
droid plugin install humanize-rs@humanize-rs
```

If the marketplace name differs in your environment, run `droid plugin marketplace list` and use that marketplace name in `plugin@marketplace`.

## What Gets Installed

The shared plugin package includes:

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
droid plugin list
```
