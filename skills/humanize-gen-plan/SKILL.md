---
name: humanize-gen-plan
description: Generate a structured implementation plan from a draft document. The host should orchestrate relevance checks, clarifications, and final authoring, while the CLI handles validation and scaffold preparation.
type: flow
user-invocable: false
---

# Humanize Generate Plan

Transforms a rough draft document into a well-structured implementation plan with clear goals, acceptance criteria (AC-X format), path boundaries, and feasibility suggestions.

## Runtime Command

All command examples below use the `humanize` CLI available on `PATH`:

```bash
humanize
```

## Workflow

```mermaid
flowchart TD
    BEGIN([BEGIN]) --> VALIDATE["Validate input/output paths<br/>Run: humanize gen-plan --prepare-only --input &lt;draft&gt; --output &lt;plan&gt;"]
    VALIDATE --> CHECK{Validation passed?}
    CHECK -->|No| REPORT_ERROR["Report validation error<br/>Stop"]
    REPORT_ERROR --> END_FAIL([END])
    CHECK -->|Yes| READ_DRAFT[Read input draft file and prepared plan scaffold]
    READ_DRAFT --> CHECK_RELEVANCE{"Is draft relevant to<br/>this repository?"}
    CHECK_RELEVANCE -->|No| REPORT_IRRELEVANT["Report: Draft not related to repo<br/>Stop"]
    REPORT_IRRELEVANT --> END_FAIL
    CHECK_RELEVANCE -->|Yes| ANALYZE["Analyze draft for:<br/>- Clarity<br/>- Consistency<br/>- Completeness<br/>- Functionality"]
    ANALYZE --> HAS_ISSUES{Issues found?}
    HAS_ISSUES -->|Yes| RESOLVE["Engage user to resolve issues<br/>via AskUserQuestion"]
    RESOLVE --> ANALYZE
    HAS_ISSUES -->|No| CHECK_METRICS{"Has quantitative<br/>metrics?"}
    CHECK_METRICS -->|Yes| CONFIRM_METRICS["Confirm metrics with user:<br/>Hard requirement or trend?"]
    CONFIRM_METRICS --> GEN_PLAN
    CHECK_METRICS -->|No| GEN_PLAN["Generate structured plan content<br/>using repository context and clarified draft"]
    GEN_PLAN --> WRITE["Write plan to output file<br/>using Edit tool to preserve draft"]
    WRITE --> REVIEW["Review complete plan<br/>Check for inconsistencies"]
    REVIEW --> INCONSISTENT{Inconsistencies?}
    INCONSISTENT -->|Yes| FIX[Fix inconsistencies]
    FIX --> REVIEW
    INCONSISTENT -->|No| CHECK_LANG{"Multiple languages?"}
    CHECK_LANG -->|Yes| UNIFY[Ask user to unify language]
    UNIFY --> REPORT_SUCCESS
    CHECK_LANG -->|No| REPORT_SUCCESS["Report success:<br/>- Plan path<br/>- AC count<br/>- Language unified?"]
    REPORT_SUCCESS --> END_SUCCESS([END])
```

## Required Sequence

### 1. Validate and Prepare

Run the CLI in scaffold mode:

```bash
humanize gen-plan --prepare-only --input <path/to/draft.md> --output <path/to/plan.md>
```

This step should:
- validate input and output paths
- create the output file from the embedded plan template
- append the original draft under `--- Original Design Draft Start ---`

If this command exits non-zero, stop and report the error directly.

### 2. Check Repository Relevance

After scaffold creation succeeds:

1. Read the draft file
2. Use the Task tool to invoke the `humanize:draft-relevance-checker` agent
3. If the result is not relevant, report that and stop

### 3. Analyze the Draft

Analyze the draft for:
- clarity
- consistency
- completeness
- functionality

Use repository exploration where needed to understand affected code paths and dependencies.

### 4. Resolve Ambiguities

If issues are found:
- use AskUserQuestion to resolve them
- preserve the original draft content
- treat answers as additive clarifications, not replacements

For quantitative metrics:
- explicitly ask whether each metric is a **hard requirement** or a **trend / optimization direction**

### 5. Generate Plan Content

Generate plan content that includes:
- goal description
- acceptance criteria with positive and negative tests
- path boundaries
- feasibility hints
- dependencies and milestones
- implementation notes

The generated plan must preserve all meaningful information from the original draft plus all clarifications.

### 6. Update the Prepared Output File

Use the Edit tool to update the prepared output file:
- replace template placeholders with the generated plan
- keep the original draft section intact at the bottom
- review the full file for consistency

### 7. Optional Language Unification

If the resulting plan mixes languages:
- ask the user whether to unify the language
- if yes, translate while preserving meaning and structure

## Validation Exit Codes

`humanize gen-plan --prepare-only` uses these exit codes:

| Exit Code | Meaning |
|-----------|---------|
| 0 | Success - continue |
| 1 | Input file not found |
| 2 | Input file is empty |
| 3 | Output directory does not exist |
| 4 | Output file already exists |
| 5 | No write permission |
| 6 | Invalid arguments |
| 7 | Plan template file not found |

## Important Note

The full `humanize gen-plan` command still exists for terminal-only workflows, but inside Claude Code the preferred behavior is this host-driven flow:
- CLI for deterministic validation and scaffold preparation
- host reasoning for analysis, clarification, and final authoring
