# Install for Claude Code

This repository now uses the native Rust `humanize` binary as the runtime.

## Local Development

From the repository root:

```bash
export CLAUDE_PLUGIN_ROOT="$PWD"
export CLAUDE_PROJECT_DIR="$PWD"
cargo build --release
cargo run -- install --plugin-root "$PWD"
```

This installs the executable at:

```text
bin/humanize
```

Claude hook configuration is in:

```text
hooks/hooks.json
```

and points directly at `bin/humanize`.

## Validate

```bash
./bin/humanize --help
cat hooks/hooks.json
```
