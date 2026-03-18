---
name: humanize-rlcr
description: Run RLCR with hook-equivalent enforcement using the native `humanize` binary.
type: flow
user-invocable: false
---

# Humanize RLCR Loop

Use the native binary directly.

Requirement: `humanize` must be available on `PATH`.

## Setup

```bash
humanize setup rlcr $ARGUMENTS
```

## Per-Round Gate

```bash
humanize gate rlcr
```

If needed:

```bash
humanize gate rlcr --session-id "$CLAUDE_SESSION_ID"
humanize gate rlcr --transcript-path "$CLAUDE_TRANSCRIPT_PATH"
```

Gate result meanings:

- `0`: allowed
- `10`: blocked, follow the returned instructions
- `20`: runtime / infrastructure error

## Cancel

```bash
humanize cancel rlcr
```
