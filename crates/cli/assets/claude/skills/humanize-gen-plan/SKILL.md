---
name: humanize-gen-plan
description: Generate a structured implementation plan from a draft using `humanize gen-plan`.
type: flow
user-invocable: false
---

# Humanize Generate Plan

Requirement: `humanize` must be available on `PATH`.

Run:

```bash
humanize gen-plan --input <draft.md> --output <plan.md>
```

The native command handles:

1. IO validation
2. repository relevance check
3. draft analysis
4. interactive clarification when needed
5. final plan generation

The generated plan preserves the original draft section at the bottom.
