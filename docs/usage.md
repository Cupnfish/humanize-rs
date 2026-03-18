# Usage Guide

This guide describes the native Rust `humanize` workflow.

Humanize is designed to run as:

- a Rust binary on `PATH`
- a host plugin installed into Claude Code or Droid by `humanize init`
- a Codex-backed review workflow

## Core Commands

### Generate Plan

```bash
humanize gen-plan --input draft.md --output docs/plan.md
```

### Start RLCR

```bash
humanize setup rlcr docs/plan.md
```

### Resume RLCR

```bash
humanize resume rlcr
```

### RLCR Stop / Gate

```bash
printf '{}' | humanize stop rlcr
humanize gate rlcr
```

### Start PR Loop

```bash
humanize setup pr --claude
humanize setup pr --codex
humanize setup pr --claude --codex
```

### Resume PR Loop

```bash
humanize resume pr
```

### Stop PR Loop

```bash
printf '{}' | humanize stop pr
```

### Cancel Loops

```bash
humanize cancel rlcr
humanize cancel pr
```

### Monitor

```bash
humanize monitor rlcr
humanize monitor pr
humanize monitor skill
```

Snapshot mode:

```bash
humanize monitor rlcr --once
```

### Ask Codex

```bash
humanize ask-codex "Explain the latest review result"
```

## Runtime State

Humanize stores runtime state under `.humanize/`:

- `.humanize/rlcr/`
- `.humanize/pr-loop/`
- `.humanize/skill/`

## Host Plugin Sync

`humanize init --global` delegates installation to Claude Code's plugin manager.
`humanize init --global --target droid` delegates installation to Droid's plugin manager.

The binary embeds `prompt-template/` internally.
After each init run, Humanize stores a sync stamp so later CLI runs can warn if the host plugin was last synced by a different CLI version.
Use `humanize doctor` to review this state on demand.
