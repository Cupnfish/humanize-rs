# Install for Claude Code

This repository exposes the Claude plugin package as `humanize-rs`.
The runtime executable itself remains `humanize` on `PATH`.

## Local Development

From the repository root:

```bash
export CLAUDE_PLUGIN_ROOT="$PWD"
export CLAUDE_PROJECT_DIR="$PWD"
cargo build --release
humanize install --target claude --plugin-root "$PWD"
```

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
