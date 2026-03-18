---
description: "Resume the active PR loop from existing .humanize state"
allowed-tools:
  - "Bash(humanize resume pr)"
hide-from-slash-command-tool: "true"
---

# Resume PR Loop

Run:

```bash
humanize resume pr
```

If an active PR loop exists, continue from the preserved `.humanize/pr-loop/...` state instead of starting a new loop.
