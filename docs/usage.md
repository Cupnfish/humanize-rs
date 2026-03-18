# Usage Guide

This guide describes the native Rust `humanize` workflow.

Humanize is designed to run as:

- a Rust binary on `PATH`
- a shared plugin package installed into Claude Code or Droid
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

## Plugin Assets

The shared plugin package uses:

- `hooks/hooks.json`
- `commands/`
- `agents/`
- `skills/`

The binary embeds `prompt-template/` internally.
