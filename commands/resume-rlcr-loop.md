---
description: "Resume active RLCR loop"
allowed-tools: ["Bash(humanize resume rlcr)"]
hide-from-slash-command-tool: "true"
---

# Resume RLCR Loop

To resume the active RLCR loop:

1. Run the resume command:

```bash
humanize resume rlcr
```

2. Handle the result:
   - If the first line is `NO_LOOP` or `NO_ACTIVE_LOOP`: Say "No active RLCR loop found."
   - If the first line is `MALFORMED_STATE`: Surface that error and stop
   - If the command succeeds: Continue to step 3

3. Use the command output as the authoritative resume state. The command prints:
   - loop metadata (`Loop Directory`, `State File`, `Status`, `Phase`, `Round`, `Plan File`, `Start Branch`, `Base Branch`)
   - the `Action File`
   - `Session Rebind: armed`
   - the action content you should continue from

## Phase Handling

The command resumes the RLCR loop by detecting the current phase and replaying the correct artifact:

- **`implementation`**: Replay `round-N-prompt.md` when it exists, or fall back to the latest goal-tracker-based resume instructions
- **`review-fix`**: Replay the current round prompt while the loop is in review correction mode
- **`review-pending`**: No local fix prompt is pending. Continue working in the host, then stop again so Humanize can retry Codex review
- **`finalize`**: Resume the Finalize Phase using `finalize-summary.md`. If the file does not exist yet, the command creates it first

**Key principle**: Resume does not start a new loop. The command finds the newest active RLCR loop under `.humanize/rlcr/`, clears the stale session binding, arms `.humanize/.pending-session-id`, and replays the current action file so work continues from the existing state.
