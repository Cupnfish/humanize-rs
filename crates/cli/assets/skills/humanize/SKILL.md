---
name: humanize
description: Iterative development with AI review. Provides RLCR, PR review automation, plan generation, monitor, and Codex consultation via the native Rust `humanize` binary.
user-invocable: false
---

# Humanize

Humanize creates a feedback loop where AI implements a plan while another AI independently reviews the work.

## Runtime Root

The installer hydrates this skill with an absolute runtime root path:

```bash
{{HUMANIZE_RUNTIME_ROOT}}
```

The runtime root provides prompt templates and related assets, while the `humanize` executable is expected to be available on `PATH`.

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize
```

## Core Workflows

### RLCR

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize setup rlcr path/to/plan.md
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize gate rlcr
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize cancel rlcr
```

### PR Loop

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize setup pr --claude
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize setup pr --codex
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize stop pr
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize cancel pr
```

### Plan Generation

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize gen-plan --input draft.md --output docs/plan.md
```

### Ask Codex

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize ask-codex "your question"
```

### Monitor

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize monitor rlcr
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize monitor pr
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize monitor skill
```

## Important Rules

1. Do not manually edit loop state files.
2. Use the native `gate rlcr` command for hook-equivalent RLCR enforcement in skill mode.
3. Do not replace RLCR/PR loop transitions with ad-hoc shell scripts.
4. Treat `.humanize/` as the runtime state directory and the current source of truth.

## Prerequisites

- `codex` CLI
- `gh` CLI for PR loop workflows

## Data Locations

Humanize stores runtime state in:

```text
.humanize/rlcr/
.humanize/pr-loop/
.humanize/skill/
```
