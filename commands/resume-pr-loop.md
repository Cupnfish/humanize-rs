---
description: "Resume active PR loop"
allowed-tools: ["Bash(humanize resume pr)"]
hide-from-slash-command-tool: "true"
---

# Resume PR Loop

To resume the active PR loop:

1. Run the native resume command:

```bash
humanize resume pr
```

2. Handle the result:
   - If the command reports `No active PR loop found.` or `No active PR loop state file found.`: Say "No active PR loop found."
   - If the command succeeds: Continue to step 3

3. Use the command output as the authoritative resume state. The native command prints:
   - loop metadata (`Loop Directory`, `State File`, `Status`, `Phase`, `Round`, `PR Number`, configured bots, active bots)
   - an `Action File`
   - the action content you should continue from

## Phase Handling

The native command resumes the PR loop by replaying the most relevant artifact for the current state:

- **`bot-feedback`**: If `round-N-pr-feedback.md` exists, replay that feedback file and continue addressing the comments
- **`initial`**: If the loop is still at round 0 and `round-0-prompt.md` exists, replay the initial startup prompt
- **`active`**: Otherwise, continue from the current round using the existing resolve/comment files shown in the output

**Key principle**: Resume does not start a new PR loop. The native command finds the newest active PR loop under `.humanize/pr-loop/` and replays the current action file so work continues from the existing state.
