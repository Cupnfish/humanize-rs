---
name: humanize-rlcr
description: Start RLCR (Ralph-Loop with Codex Review) with hook-equivalent enforcement from skill mode by reusing the existing native `humanize gate rlcr` logic.
type: flow
user-invocable: false
---

# Humanize RLCR Loop (Hook-Equivalent)

Use this flow to run RLCR in environments without native hooks.  
Do not re-implement review logic manually. Always call the RLCR stop gate wrapper through the CLI:

```bash
humanize gate rlcr
```

The gate command routes through the same stop-hook-compatible logic, so skill-mode behavior stays aligned with hook-mode behavior.

## Runtime Command

All commands below assume the `humanize` CLI is available on `PATH`.

## Required Sequence

### 1. Setup

Start the loop with the setup command:

```bash
humanize setup rlcr $ARGUMENTS
```

If setup exits non-zero, stop and report the error.

### 2. Work Round

For each round:

1. Read current loop prompt from `.humanize/rlcr/<timestamp>/round-<N>-prompt.md` (or finalize prompt files when in finalize phase).
2. Implement required changes.
3. Commit changes.
4. Write the required summary file:
   - normal phase: `.humanize/rlcr/<timestamp>/round-<N>-summary.md`
   - finalize phase: `.humanize/rlcr/<timestamp>/finalize-summary.md`
5. Run the gate command:

```bash
humanize gate rlcr
humanize gate rlcr --session-id "$CLAUDE_SESSION_ID"
humanize gate rlcr --transcript-path "$CLAUDE_TRANSCRIPT_PATH"
```

6. Handle gate result:
   - `0`: loop is allowed to exit
   - `10`: blocked by RLCR logic; follow returned instructions exactly and continue the next round
   - `20`: infrastructure/runtime error; report the error and stop

## What This Enforces

By routing through the native gate logic, this skill enforces:

- state/schema validation (`current_round`, `max_iterations`, `review_started`, `base_branch`, etc.)
- branch consistency checks
- plan-file integrity checks (when applicable)
- incomplete Task/Todo blocking
- git-clean requirement before exit
- `--push-every-round` unpushed-commit blocking
- summary presence checks
- max-iteration handling
- full-alignment rounds (`--full-review-round`)
- strict `COMPLETE` / `STOP` marker handling
- review-phase transition guards
- code-review gating on `[P0-9]` markers
- hard blocking on codex review failure or empty output
- open-question handling when `ask_codex_question=true`

## Critical Rules

1. Never manually edit `state.md` or `finalize-state.md`.
2. Never skip a blocked gate result by declaring completion manually.
3. Never run ad-hoc `codex exec` or `codex review` in place of the gate for phase transitions.
4. Always use loop-generated files (`round-*-prompt.md`, `round-*-review-result.md`) as source of truth.

## Options

Pass these through `humanize setup rlcr`:

| Option | Description | Default |
|--------|-------------|---------|
| `path/to/plan.md` | Plan file path | Required unless `--skip-impl` |
| `--plan-file <path>` | Explicit plan path | - |
| `--track-plan-file` | Enforce tracked plan immutability | false |
| `--max` or `--max-iterations N` | Maximum iterations | 42 |
| `--codex-model MODEL:EFFORT` | Codex model and effort for `codex exec` | `gpt-5.4:xhigh` |
| `--codex-timeout SECONDS` | Codex timeout | 5400 |
| `--base-branch BRANCH` | Base for review phase | auto-detect |
| `--full-review-round N` | Full alignment interval | 5 |
| `--skip-impl` | Start directly in review path | false |
| `--push-every-round` | Require push each round | false |
| `--claude-answer-codex` | Let Claude answer open questions directly | false |
| `--agent-teams` | Enable agent teams mode | false |

Review phase `codex review` runs with `gpt-5.4:high`.

## Usage

```bash
# Start with plan file
humanize setup rlcr path/to/plan.md

# Review-only mode
humanize setup rlcr --skip-impl
```

## Cancel

```bash
humanize cancel rlcr
```
