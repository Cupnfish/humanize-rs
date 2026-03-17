---
description: "Start iterative RLCR loop with the native humanize binary"
argument-hint: "[path/to/plan.md | --plan-file path/to/plan.md] [--max-iterations N] [--codex-model MODEL:EFFORT] [--codex-timeout SECONDS] [--track-plan-file] [--push-every-round] [--base-branch BRANCH] [--full-review-round N] [--skip-impl] [--claude-answer-codex] [--agent-teams]"
allowed-tools:
  - "Bash(humanize setup rlcr:*)"
hide-from-slash-command-tool: "true"
---

# Start RLCR Loop

Run:

```bash
humanize setup rlcr $ARGUMENTS
```

The native command initializes `.humanize/rlcr/<timestamp>/` and prepares:

- `state.md`
- `plan.md`
- `goal-tracker.md`
- `round-0-prompt.md`

Use the native gate command for each round in skill-mode or non-hook environments:

```bash
humanize gate rlcr
```
