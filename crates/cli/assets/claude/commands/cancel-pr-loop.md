---
description: "Cancel the active PR loop using the native humanize binary"
allowed-tools:
  - "Bash(humanize cancel pr)"
hide-from-slash-command-tool: "true"
---

# Cancel PR Loop

Run:

```bash
humanize cancel pr
```

If the command reports no active PR loop, tell the user there is nothing to cancel.
If it succeeds, report the cancellation message and keep the preserved loop directory for reference.
