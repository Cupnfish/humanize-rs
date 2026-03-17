---
description: "Start PR review loop with bot monitoring using the native humanize binary"
argument-hint: "--claude|--codex [--max-iterations N] [--codex-model MODEL:EFFORT] [--codex-timeout SECONDS]"
allowed-tools:
  - "Bash(humanize setup pr:*)"
hide-from-slash-command-tool: "true"
---

# Start PR Loop

Run:

```bash
humanize setup pr $ARGUMENTS
```

The native command initializes `.humanize/pr-loop/<timestamp>/` and prepares:

- `state.md`
- `goal-tracker.md`
- `round-0-pr-comment.md`
- `round-0-prompt.md`

At least one of `--claude` or `--codex` is required.
