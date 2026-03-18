# Install for Claude Code

This repository exposes the Claude plugin package as `humanize-rs`.
The runtime executable itself remains `humanize` on `PATH`.

## Local Development

From the repository root:

```bash
cargo build --release
humanize install --target claude
```

Default runtime root:

- Windows: `%APPDATA%\\humanize-rs`
- macOS: `~/Library/Application Support/humanize-rs`
- Linux/Unix: `${XDG_DATA_HOME:-~/.local/share}/humanize-rs`

If your Claude environment needs an explicit runtime root, point it at that installed directory.

Claude hook configuration is in:

```text
hooks/hooks.json
```

and points directly at `humanize` on `PATH`.

## Validate

```bash
humanize --help
cat hooks/hooks.json
```
