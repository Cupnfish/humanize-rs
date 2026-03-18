# Install for Droid

Humanize uses a two-part deployment model:

- `humanize` on `PATH`
- Droid-side plugin installed by Droid's native plugin manager

## Prerequisites

```bash
humanize --help
codex --version
droid --version
```

## Install

Recommended native install:

```bash
humanize init --global --target droid
```

This command:

- adds the Humanize marketplace source if needed
- installs or updates `humanize-rs` in user scope
- records the CLI version used for the sync

The `humanize` binary itself is not bundled into the plugin.
It must already be available on `PATH`.

## Version Sync

`humanize init --global --target droid` writes a sync stamp under `~/.factory/`.
When the `humanize` CLI version changes later, CLI commands warn if the Droid plugin was last synced by a different version.
`humanize doctor --target droid` summarizes this state in one place.

Project maintainers should use:

```bash
cargo xtask sync-version
cargo xtask verify-version-sync
```

## Validate

```bash
humanize --help
humanize init --global --target droid --show
humanize doctor --target droid
```
