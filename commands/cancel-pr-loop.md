---
description: "Cancel active PR loop"
allowed-tools: ["Bash(humanize cancel pr)", "Bash(humanize cancel pr --force)"]
hide-from-slash-command-tool: "true"
---

# Cancel PR Loop

To cancel the active PR loop:

1. Run the native cancel command:

```bash
humanize cancel pr
```

2. Handle the result:
   - If the command reports `No active PR loop found.`: Say "No active PR loop found."
   - If the first line of stdout is `CANCELLED`: Report the cancellation message from the output

**Key principle**: The native command handles all cancellation logic. A PR loop is active if `state.md` exists in the newest PR loop directory (`.humanize/pr-loop/`).

The loop directory with comments, resolution summaries, and state information will be preserved for reference. The command writes `.cancel-requested` and renames `state.md` to `cancel-state.md`.

**Note**: This command only affects PR loops. RLCR loops (`.humanize/rlcr/`) are not affected. Use `/humanize-cancel-rlcr-loop` to cancel RLCR loops.
