---
description: "Recover and continue active PR loop"
allowed-tools: ["Bash(humanize resume pr)"]
hide-from-slash-command-tool: "true"
---

# Resume PR Loop

Execute the resume command and treat its output as the authoritative PR-loop state:

```bash
humanize resume pr
```

## Handle Result

Check the first line of output:

- `NO_LOOP` or `NO_ACTIVE_LOOP`: Say "No active PR loop found."
- `MALFORMED_STATE`: Surface the error and stop
- Otherwise: Continue from the printed resume state below. Do not manually re-derive the phase from `.humanize/pr-loop/`

## Continue From Output

The command already chooses the most relevant artifact for the active PR loop. Use the output as-is:

- loop metadata (`Loop Directory`, `State File`, `Status`, `Phase`, `Round`, `PR Number`, configured bots, active bots)
- `Action File`
- the action content to continue from

Treat the printed action content as the next task. Do not start a new PR loop just because the current round is not round 0.

## Phase Meanings

The command can surface these PR phases:

- **`bot-feedback`**: Replay `round-N-pr-feedback.md` and continue addressing the remaining issues
- **`initial`**: The loop is still at round 0, so replay the initial startup prompt
- **`active`**: Continue the current round from the printed resolve/comment artifacts
