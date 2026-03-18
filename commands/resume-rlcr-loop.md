---
description: "Resume the active RLCR loop from existing .humanize state"
allowed-tools:
  - "Bash(humanize resume rlcr)"
hide-from-slash-command-tool: "true"
---

# Resume RLCR Loop

Run:

```bash
humanize resume rlcr
```

If an active RLCR loop exists, use the returned prompt and status summary to continue from the preserved `.humanize/rlcr/...` state instead of starting a new loop.
