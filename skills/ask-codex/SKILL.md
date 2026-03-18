---
name: ask-codex
description: Consult Codex as an independent expert using the `humanize ask-codex` command.
argument-hint: "[--model MODEL] [--effort EFFORT] [--timeout SECONDS] [question or task]"
---

# Ask Codex

Run:

```bash
humanize ask-codex $ARGUMENTS
```

Requirements:

- `humanize` must be available on `PATH`
- the command prints the Codex response to `stdout`

Outputs are stored under `.humanize/skill/<timestamp>/`.
