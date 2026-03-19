---
description: "Cancel active RLCR loop"
allowed-tools:
  - "Bash(humanize cancel rlcr)"
  - "Bash(humanize cancel rlcr --force)"
  - "AskUserQuestion"
hide-from-slash-command-tool: "true"
---

# Cancel RLCR Loop

To cancel the active loop:

1. Run the native cancel command:

```bash
humanize cancel rlcr
```

2. Handle the result:
   - If the command reports `No active RLCR loop found.`: Say "No active RLCR loop found."
   - If the first line of stdout is `CANCELLED`: Report the cancellation message from the output
   - If the first line of stdout is `FINALIZE_NEEDS_CONFIRM`: The loop is in Finalize Phase. Continue to step 3

3. **If FINALIZE_NEEDS_CONFIRM**:
   - Use AskUserQuestion to confirm cancellation with these options:
     - Question: "The loop is currently in Finalize Phase. After this phase completes, the loop will end without returning to Codex review. Are you sure you want to cancel now?"
     - Header: "Cancel?"
     - Options:
       1. Label: "Yes, cancel now", Description: "Cancel the loop immediately, finalize-state.md will be renamed to cancel-state.md"
       2. Label: "No, let it finish", Description: "Continue with the Finalize Phase, the loop will complete normally"
   - **If user chooses "Yes, cancel now"**:
     - Run: `humanize cancel rlcr --force`
     - Report the cancellation message from the output
   - **If user chooses "No, let it finish"**:
     - Report: "Understood. The Finalize Phase will continue. Once complete, the loop will end normally."

**Key principle**: The native command handles all cancellation logic. A loop is active if `state.md` (normal loop) or `finalize-state.md` (Finalize Phase) exists in the newest loop directory.

The loop directory with summaries, review results, and state information will be preserved for reference. The command writes `.cancel-requested` and renames the active state file to `cancel-state.md`.
