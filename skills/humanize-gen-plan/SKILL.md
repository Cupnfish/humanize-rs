---
name: humanize-gen-plan
description: Generate a structured implementation plan from a draft document using the native Rust `humanize gen-plan` flow.
type: flow
user-invocable: false
---

# Humanize Generate Plan

Transforms a rough draft document into a structured implementation plan.

## Runtime Root

The installer hydrates this skill with an absolute runtime root path:

```bash
{{HUMANIZE_RUNTIME_ROOT}}
```

The runtime root provides prompt templates and related assets, while the `humanize` executable is expected to be available on `PATH`.

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize
```

## Flow

```mermaid
flowchart TD
    BEGIN([BEGIN]) --> RUN[Run `CLAUDE_PLUGIN_ROOT={{HUMANIZE_RUNTIME_ROOT}} humanize gen-plan --input ... --output ...`]
    RUN --> CHECK{Succeeded?}
    CHECK -->|No| REPORT_ERROR[Report the validation / clarification / runtime error]
    CHECK -->|Yes| REPORT_SUCCESS[Report the generated plan path and key outputs]
```

## Command

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize gen-plan --input <draft.md> --output <plan.md>
```

## What The Native Command Does

The Rust `gen-plan` flow:

1. validates input/output paths
2. checks whether the draft is relevant to the current repository
3. analyzes the draft for ambiguities and quantitative metrics
4. asks clarification questions in interactive terminals when needed
5. generates the final plan
6. preserves the original draft section at the bottom of the plan file

## Usage

```bash
/flow:humanize-gen-plan
```

Or load the skill without auto-execution:

```bash
/skill:humanize-gen-plan
```
