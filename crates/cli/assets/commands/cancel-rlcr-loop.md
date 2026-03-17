---
description: "Cancel the active RLCR loop using the native humanize binary"
allowed-tools:
  - "Bash(humanize cancel rlcr)"
  - "Bash(humanize cancel rlcr --force)"
  - "AskUserQuestion"
hide-from-slash-command-tool: "true"
---

# Cancel RLCR Loop

Run:

```bash
humanize cancel rlcr
```

If the loop is in Finalize Phase and force confirmation is required, ask the user whether to force-cancel.
If they confirm, run:

```bash
humanize cancel rlcr --force
```
