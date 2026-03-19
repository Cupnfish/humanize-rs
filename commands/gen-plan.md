---
description: "Generate implementation plan from draft document"
argument-hint: "--input <path/to/draft.md> --output <path/to/plan.md>"
allowed-tools:
  - "Bash(humanize gen-plan:*)"
  - "Read"
  - "Glob"
  - "Grep"
  - "Task"
  - "Write"
  - "AskUserQuestion"
hide-from-slash-command-tool: "true"
---

# Generate Plan from Draft

This command transforms a user's draft document into a well-structured implementation plan with clear goals, acceptance criteria (AC-X format), path boundaries, and feasibility suggestions.

## Workflow Overview

1. **IO Validation**: Validate input and output paths
2. **Relevance Check**: Verify draft is relevant to the repository
3. **Draft Analysis**: Analyze draft for clarity, consistency, completeness, and functionality
4. **Issue Resolution**: Engage the user when clarifications or metric confirmations are needed
5. **Plan Generation**: Generate the structured plan.md
6. **Write and Complete**: Write output file and report results

---

## Execute Native Flow

Run the native command:

```bash
humanize gen-plan $ARGUMENTS
```

The native command performs the workflow above internally.

---

## Phase 1: IO Validation

Before doing any Codex work, the native command validates:

- input file exists
- input file is not empty
- output directory exists
- output file does not already exist
- embedded plan template is available
- `codex` is installed

If the command exits with a validation error, report the specific error and stop.

---

## Phase 2: Relevance Check

After IO validation passes, the native command checks if the draft is relevant to this repository.

> **Note**: Be lenient. As long as the draft is not clearly unrelated to the current project, it passes.

If the command reports that the draft does not appear to be related to this repository:
- Report that result to the user
- Show the reason from the relevance check
- Stop the command

---

## Phase 3: Draft Analysis

The native command analyzes the draft for potential issues across these dimensions:

1. **Clarity**: Is the draft's intent and scope clearly expressed?
2. **Consistency**: Does the draft contradict itself?
3. **Completeness**: Are there missing dependencies, side effects, or edge cases?
4. **Functionality**: Would the proposed design actually work in this repository?

The native command may also detect:
- quantitative metrics that need hard-vs-trend confirmation
- mixed-language content that may need language unification
- additional notes that should inform the generated plan

---

## Phase 4: Issue Resolution

> **Critical**: The draft document contains the most valuable human input. During issue resolution, NEVER discard or override any original draft content. All clarifications should be treated as incremental additions that supplement the draft, not replacements.

If the native command asks clarification questions:
- answer them interactively with the user
- treat each answer as an addition to the draft, not a replacement

If the native command asks to classify a quantitative metric:
- use `hard` when the number is a strict success requirement
- use `trend` when the number describes a direction of optimization

If the command is run in a non-interactive environment and reports that clarification or metric confirmation is required:
- tell the user the command must be rerun in an interactive session
- do not invent answers on the user's behalf

If mixed-language content is detected, the native command may ask whether to:
- `keep`
- `english`
- `chinese`

---

## Phase 5: Plan Generation

The native command generates the plan using the embedded template. The result should:

- preserve all meaningful information from the input draft
- use AC-X or AC-X.Y acceptance criteria
- include positive and negative tests for each acceptance criterion
- include path boundaries, feasibility hints, dependencies, and implementation notes
- preserve the original draft in the final file

### Plan Structure

The native command follows the embedded plan template and generates a plan with this structure:

```markdown
# <Plan Title>

## Goal Description
<Clear, direct description of what needs to be accomplished>

## Acceptance Criteria

- AC-1: <First criterion>
  - Positive Tests (expected to PASS):
    - <Test case that should succeed when criterion is met>
  - Negative Tests (expected to FAIL):
    - <Test case that should fail/be rejected when working correctly>
- AC-2: <Second criterion>
...

## Path Boundaries

### Upper Bound (Maximum Acceptable Scope)
<Affirmative description of the most comprehensive acceptable implementation>

### Lower Bound (Minimum Acceptable Scope)
<Affirmative description of the minimum viable implementation>

### Allowed Choices
- Can use: <acceptable approaches>
- Cannot use: <prohibited approaches>

## Feasibility Hints and Suggestions

### Conceptual Approach
<One possible implementation path>

### Relevant References
- <path/to/relevant/component> - <brief description>

## Dependencies and Sequence

### Milestones
1. <Milestone 1>: <Description>
2. <Milestone 2>: <Description>

## Implementation Notes
```

### Generation Rules

The native command should follow these rules internally:

1. Use Milestone, Phase, Step, and Section terminology. Do not use time estimates.
2. Reference code by path only, never by line number.
3. Keep path boundaries affirmative and describe what is acceptable.
4. Treat deterministic designs as fixed constraints when the draft requires them.
5. Preserve all draft information and treat clarifications as additive.
6. Keep plan-specific terminology such as `AC-`, `Milestone`, `Step`, and `Phase` out of production code and comments.

---

## Phase 6: Write and Complete

If the command succeeds, report to the user:

- the path to the generated plan
- a brief summary of what was included
- whether language unification was performed

---

## Error Handling

If the command fails during generation, clarification, metric confirmation, or language unification:
- report the specific error back to the user
- stop without pretending the plan was generated
