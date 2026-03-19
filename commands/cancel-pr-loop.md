---
description: "Cancel active PR loop"
allowed-tools: ["Bash(humanize cancel pr)", "Bash(humanize cancel pr --force)"]
hide-from-slash-command-tool: "true"
---

# Cancel PR Loop

To cancel the active PR loop:

1. Run the cancel command:

```bash
humanize cancel pr
```

2. Check the first line of output:
   - **NO_LOOP** or **NO_ACTIVE_LOOP**: Say "No active PR loop found."
   - **CANCELLED**: Report the cancellation message from the output

**Key principle**: The command handles all cancellation logic. A PR loop is active if `state.md` exists in the newest PR loop directory (.humanize/pr-loop/).

The loop directory with comments, resolution summaries, and state information will be preserved for reference. The command writes `.cancel-requested` and renames `state.md` to `cancel-state.md`.

**Note**: This command only affects PR loops. RLCR loops (.humanize/rlcr/) are not affected. Use `/humanize-rs:cancel-rlcr-loop` to cancel RLCR loops.
