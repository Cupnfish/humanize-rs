---
name: humanize
description: Iterative development with RLCR, PR loop automation, plan generation, monitor, and Codex consultation via the native `humanize` binary.
user-invocable: false
---

# Humanize

Requirement: `humanize` must be available on `PATH`.

Core commands:

```bash
humanize setup rlcr <plan>
humanize resume rlcr
humanize gate rlcr
humanize setup pr --claude|--codex
humanize resume pr
humanize stop pr
humanize cancel rlcr
humanize cancel pr
humanize gen-plan --input draft.md --output docs/plan.md
humanize ask-codex "question"
humanize monitor rlcr
humanize monitor pr
humanize monitor skill
```

Requirements:

- `humanize` must be available on `PATH`
- `codex` is required for Codex-backed flows
- `gh` is required for PR loop flows

Runtime state is stored under `.humanize/`.

If a host session is lost but `.humanize/` still exists, resume the active loop instead of starting a new one:

```bash
humanize resume rlcr
humanize resume pr
```
