---
name: humanize
description: Iterative development with AI review. Provides RLCR (Ralph-Loop with Codex Review) for implementation planning and code review loops, plus PR review automation with bot monitoring.
user-invocable: false
---

# Humanize - Iterative Development with AI Review

Humanize creates a feedback loop where AI implements your plan while another AI independently reviews the work, ensuring quality through continuous refinement.

## Runtime Command

All command examples below use the `humanize` CLI available on `PATH`:

```bash
humanize
```

## Core Philosophy

**Iteration over Perfection**: Instead of expecting perfect output in one shot, Humanize leverages an iterative feedback loop where:
- AI implements your plan
- Another AI independently reviews progress
- Issues are caught and addressed early
- Work continues until all acceptance criteria are met

## Available Workflows

### 1. RLCR Loop - Iterative Development with Review

The RLCR (Ralph-Loop with Codex Review) loop has two phases:

**Phase 1: Implementation**
- AI works on the implementation plan
- AI writes a summary of work completed
- Codex reviews the summary for completeness and correctness
- If issues found -> feedback loop continues
- If Codex outputs "COMPLETE" -> enters Review Phase

**Phase 2: Code Review**
- `codex review --base <branch>` checks code quality
- Issues marked with `[P0-9]` severity markers
- If issues found -> AI fixes them and continues
- If no issues -> loop completes with Finalize Phase
- In skill mode, always run `humanize gate rlcr` to enforce hook-equivalent transitions and blocking

### 2. PR Loop - Automated PR Review Handling

Automates handling of GitHub PR reviews from remote bots:

1. Detects the PR associated with the current branch
2. Fetches review comments from specified bot(s) (`--claude` and/or `--codex`)
3. AI analyzes and fixes issues identified by the bot(s)
4. Pushes changes and triggers re-review by commenting `@bot`
5. Stop Hook polls for new bot reviews (every 30s, 15min timeout per bot)
6. Local Codex validates if remote concerns are resolved
7. Loop continues until all bots approve or max iterations reached

### 3. Generate Plan - Structured Plan from Draft

Transforms a rough draft document into a structured implementation plan with:
- clear goal description
- acceptance criteria in AC-X format with TDD-style positive/negative tests
- path boundaries (upper/lower bounds, allowed choices)
- feasibility hints and conceptual approach
- dependencies and milestone sequencing

When running inside Claude Code, prefer the `humanize-gen-plan` flow/skill behavior:
- use `humanize gen-plan --prepare-only` for deterministic validation and scaffold preparation
- use host reasoning plus AskUserQuestion for clarification and final authoring

The full `humanize gen-plan` command remains available for standalone terminal workflows.

## Commands Reference

### Start RLCR Loop

```bash
# With a plan file
humanize setup rlcr path/to/plan.md

# Or without plan (review-only mode)
humanize setup rlcr --skip-impl
```

```bash
# For each round, run the RLCR gate (required)
humanize gate rlcr
```

**Common Options:**
- `--max` or `--max-iterations N` - Maximum iterations before auto-stop (default: 42)
- `--codex-model MODEL:EFFORT` - Codex model and reasoning effort for `codex exec` (default: `gpt-5.4:xhigh`)
- Review phase `codex review` uses `gpt-5.4:high`
- `--codex-timeout SECONDS` - Timeout for each Codex review (default: 5400)
- `--base-branch BRANCH` - Base branch for code review (auto-detects if not specified)
- `--full-review-round N` - Interval for full alignment checks (default: 5)
- `--skip-impl` - Skip implementation phase, go directly to code review
- `--track-plan-file` - Enforce plan-file immutability when tracked in git
- `--push-every-round` - Require git push after each round
- `--claude-answer-codex` - Let Claude answer Codex Open Questions directly (default is AskUserQuestion behavior)
- `--agent-teams` - Enable Agent Teams mode

### Cancel RLCR Loop

```bash
humanize cancel rlcr
# or force cancel during finalize phase
humanize cancel rlcr --force
```

### Start PR Loop

```bash
# Monitor claude[bot] reviews
humanize setup pr --claude

# Monitor chatgpt-codex-connector[bot] reviews
humanize setup pr --codex

# Monitor both
humanize setup pr --claude --codex
```

**Common Options:**
- `--max` or `--max-iterations N` - Maximum iterations (default: 42)
- `--codex-model MODEL:EFFORT` - Codex model for validation (default: `gpt-5.4:medium`)
- `--codex-timeout SECONDS` - Timeout for Codex validation (default: 900)

### Cancel PR Loop

```bash
humanize cancel pr
```

### Generate Plan from Draft

```bash
humanize gen-plan --prepare-only --input path/to/draft.md --output path/to/plan.md
```

After scaffold preparation, continue with host-driven analysis, clarification, and plan authoring.

### Ask Codex (One-shot Consultation)

```bash
humanize ask-codex [--model MODEL] [--effort EFFORT] [--timeout SECONDS] "your question"
```

## Goal Tracker System

The RLCR loop uses a Goal Tracker to prevent goal drift:

- **IMMUTABLE SECTION**: Ultimate Goal and Acceptance Criteria (set in Round 0, never changed)
- **MUTABLE SECTION**: Active Tasks, Completed Items, Deferred Items, Plan Evolution Log

### Key Principles

1. **Acceptance Criteria**: Each task maps to a specific AC
2. **Plan Evolution Log**: Document any plan changes with justification
3. **Explicit Deferrals**: Deferred tasks require strong justification
4. **Full Alignment Checks**: Every N rounds (default: 5), comprehensive goal alignment audit

## Important Rules

1. **Write summaries**: Always write work summary to the specified file before exiting
2. **Maintain Goal Tracker**: Keep goal-tracker.md up-to-date with progress
3. **Be thorough**: Include details about implementation, files changed, tests added
4. **No cheating**: Don't try to exit by editing state files or running cancel commands
5. **Run stop gate each round**: Use `humanize gate rlcr` instead of manual phase control
6. **Trust the process**: External review helps improve implementation quality

## Prerequisites

- `humanize` - Humanize CLI
- `codex` - OpenAI Codex CLI (for review)
- `gh` - GitHub CLI (for PR loop)
