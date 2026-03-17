---
name: humanize-rlcr
description: Start RLCR (Ralph-Loop with Codex Review) with hook-equivalent enforcement using the native Rust `humanize` binary.
type: flow
user-invocable: false
---

# Humanize RLCR Loop

Use this flow to run RLCR in environments without native hooks.
Do not re-implement phase transitions manually. Always use the native gate command.

## Runtime Root

The installer hydrates this skill with an absolute runtime root path:

```bash
{{HUMANIZE_RUNTIME_ROOT}}
```

The runtime root provides prompt templates and related assets, while the `humanize` executable is expected to be available on `PATH`.

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize
```

## Required Sequence

### 1. Setup

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize setup rlcr $ARGUMENTS
```

If setup exits non-zero, stop and report the error.

### 2. Work Round

For each round:

1. Read the current loop prompt from `.humanize/rlcr/<timestamp>/round-<N>-prompt.md`
2. Implement the required changes
3. Commit the changes
4. Write the required summary file
5. Run the native RLCR gate:

```bash
GATE_CMD=(humanize gate rlcr)
export CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}"
[[ -n "${CLAUDE_SESSION_ID:-}" ]] && GATE_CMD+=(--session-id "$CLAUDE_SESSION_ID")
[[ -n "${CLAUDE_TRANSCRIPT_PATH:-}" ]] && GATE_CMD+=(--transcript-path "$CLAUDE_TRANSCRIPT_PATH")
"${GATE_CMD[@]}"
GATE_EXIT=$?
```

6. Handle gate result:
   - `0`: loop is allowed to exit
   - `10`: blocked by RLCR logic, follow the returned instructions and continue
   - `20`: infrastructure/runtime error

## What This Enforces

By routing through the native stop/gate logic, this skill enforces:

- state/schema validation
- branch consistency checks
- plan-file integrity checks
- incomplete task blocking
- git-clean requirement before exit
- unpushed-commit blocking when configured
- summary presence checks
- max-iteration handling
- full-alignment rounds
- strict `COMPLETE` / `STOP` handling
- review-phase transition guards
- code-review gating on `[P0-9]` markers

## Options

Pass these through `humanize setup rlcr`:

| Option | Description | Default |
|--------|-------------|---------|
| `path/to/plan.md` | Plan file path | Required unless `--skip-impl` |
| `--plan-file <path>` | Explicit plan path | - |
| `--track-plan-file` | Enforce tracked plan immutability | false |
| `--max-iterations N` | Maximum iterations | 42 |
| `--codex-model MODEL:EFFORT` | Codex model and effort | gpt-5.4:xhigh |
| `--codex-timeout SECONDS` | Codex timeout | 5400 |
| `--base-branch BRANCH` | Base for review phase | auto-detect |
| `--full-review-round N` | Full alignment interval | 5 |
| `--skip-impl` | Start directly in review path | false |
| `--push-every-round` | Require push each round | false |
| `--claude-answer-codex` | Let Claude answer open questions directly | false |
| `--agent-teams` | Enable agent teams mode | false |

## Usage

```bash
/flow:humanize-rlcr path/to/plan.md
/flow:humanize-rlcr --skip-impl
/skill:humanize-rlcr
```

## Cancel

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize cancel rlcr
```
