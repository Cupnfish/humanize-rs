---
description: "Generate implementation plan from draft document using native humanize gen-plan"
argument-hint: "--input <path/to/draft.md> --output <path/to/plan.md>"
allowed-tools:
  - "Bash(humanize gen-plan:*)"
hide-from-slash-command-tool: "true"
---

# Generate Plan

Run:

```bash
humanize gen-plan $ARGUMENTS
```

The native command handles:

1. input/output validation
2. repository relevance check
3. draft analysis
4. interactive clarification when needed
5. final plan generation

If the command exits non-zero, report the validation or clarification error back to the user.
