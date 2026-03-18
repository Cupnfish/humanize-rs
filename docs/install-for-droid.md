# Install for Droid

Humanize uses a two-part deployment model:

- `humanize` on `PATH`
- Droid-side assets installed into `~/.factory`

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
# or:
humanize init --global --target droid --auto-patch
```

This installs into `~/.factory/`:

- `commands/`
- `droids/`
- `skills/`
- Humanize hook registrations in `~/.factory/settings.json`

The `humanize` binary itself is not bundled into host assets.
It must already be available on `PATH`.

## Validate

```bash
humanize --help
humanize init --global --target droid --show
```
