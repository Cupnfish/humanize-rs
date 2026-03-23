---
description: "Recover and continue active RLCR loop"
allowed-tools: ["Bash(humanize resume rlcr)"]
hide-from-slash-command-tool: "true"
---

# Resume RLCR Loop

Execute the resume command and treat its output as the authoritative loop state:

```bash
humanize resume rlcr
```

## Handle Result

Check the first line of output:

- `NO_LOOP` or `NO_ACTIVE_LOOP`: Say "No active RLCR loop found."
- `MALFORMED_STATE`: Surface the error and stop
- Otherwise: Continue from the printed resume state below. Do not inspect `.humanize/` manually to second-guess the phase.

## Continue From Output

The command already determines the active phase and chooses the correct action file. Use the output as-is:

- loop metadata (`Loop Directory`, `State File`, `Status`, `Phase`, `Round`, `Plan File`, `Start Branch`, `Base Branch`)
- `Action File`
- `Session Rebind`
- the action content to continue from

Use these rules:

- If `Session Rebind: armed`, continue directly from the printed action content
- If `Session Rebind: skipped`, the loop is legacy recovery mode. Use the printed artifacts to recover unfinished work, but do not claim the host session was rebound
- Do not start a new RLCR loop just because the current phase is unusual. `resume` is specifically for continuing the existing loop

## Phase Meanings

The command can surface these RLCR phases:

- **`implementation`**: Continue the current implementation round. If the round prompt is missing, the command may fall back to the previous review result plus the summary file target for the current round
- **`review-fix`**: The code review phase already found issues for the current round. Continue from the printed fix prompt and address those issues
- **`review-pending`**: Review phase has started, but no local fix prompt is currently pending. Continue working in the host, then stop again so Humanize can retry Codex review
- **`review-ready`**: A `--skip-impl` loop is waiting for its first review cycle. Follow the printed prompt and stop when ready for review
- **`finalize`**: Resume the Finalize Phase using `finalize-summary.md`. If the file is missing, the command creates it first
