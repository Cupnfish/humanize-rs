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
  - "Edit"
  - "AskUserQuestion"
hide-from-slash-command-tool: "true"
---

# Generate Plan from Draft

This command transforms a user's draft document into a well-structured implementation plan with clear goals, acceptance criteria (AC-X format), path boundaries, and feasibility suggestions.

## Workflow Overview

1. **IO Validation**: Validate input and output paths and prepare the output scaffold
2. **Relevance Check**: Verify draft is relevant to this repository
3. **Draft Analysis**: Analyze draft for clarity, consistency, completeness, and functionality
4. **Issue Resolution**: Engage user to clarify any issues found
5. **Plan Generation**: Generate the structured `plan.md`
6. **Write and Complete**: Update the scaffolded output file and report results

---

## Phase 1: IO Validation and Scaffold Preparation

Execute the prepare step with the provided arguments:

```bash
humanize gen-plan --prepare-only $ARGUMENTS
```

Handle failures directly:
- If the command says the input file is missing or empty, report that and stop
- If the command says the output directory is missing, report that and stop
- If the command says the output file already exists, report that and stop
- If the command says the plan template is missing, report that as a plugin/runtime configuration error and stop

On success, the output file now exists and already contains:
- the generated plan template skeleton
- the original draft appended under `--- Original Design Draft Start ---`

Continue to Phase 2.

---

## Phase 2: Relevance Check

After IO validation passes, check if the draft is relevant to this repository.

> **Note**: Do not spend too much time on this check. As long as the draft is not completely unrelated to the current project, it passes.

1. Read the input draft file to get its content
2. Use the Task tool to invoke the `humanize:draft-relevance-checker` agent:
   ```
   Task tool parameters:
   - prompt: Include the draft content and ask the agent to:
     1. Explore the repository structure (README, CLAUDE.md, main files)
     2. Analyze if the draft content relates to this repository
     3. Return either `RELEVANT: <reason>` or `NOT_RELEVANT: <reason>`
   ```

3. **If NOT_RELEVANT**:
   - Report: "The draft content does not appear to be related to this repository."
   - Show the reason from the relevance check
   - Stop the command

4. **If RELEVANT**: Continue to Phase 3

---

## Phase 3: Draft Analysis

Deeply analyze the draft for potential issues. Use Explore agents to investigate the codebase.

### Analysis Dimensions

1. **Clarity**: Is the draft's intent and goals clearly expressed?
   - Are objectives well-defined?
   - Is the scope clear?
   - Are terms and concepts unambiguous?

2. **Consistency**: Does the draft contradict itself?
   - Are requirements internally consistent?
   - Do different sections align with each other?

3. **Completeness**: Are there missing considerations?
   - Use Explore agents to investigate parts of the codebase the draft might affect
   - Identify dependencies, side effects, or related components not mentioned
   - Check if the draft overlooks important edge cases

4. **Functionality**: Does the design have fundamental flaws?
   - Would the proposed approach actually work?
   - Are there technical limitations not addressed?
   - Could the design negatively impact existing functionality?

### Exploration Strategy

Use the Task tool with `subagent_type: "Explore"` to investigate:
- Components mentioned in the draft
- Related files and directories
- Existing patterns and conventions
- Dependencies and integrations

---

## Phase 4: Issue Resolution

> **Critical**: The draft document contains the most valuable human input. During issue resolution, NEVER discard or override any original draft content. All clarifications should be treated as incremental additions that supplement the draft, not replacements. Keep track of both the original draft statements and the clarified information.

### Step 1: Resolve Analysis Issues

If any issues are found during analysis, use AskUserQuestion to clarify with the user.

For each issue category that has problems, present:
- What the issue is
- Why it matters
- Options for resolution (if applicable)

Continue this dialogue until all significant issues are resolved or acknowledged by the user.

### Step 2: Confirm Quantitative Metrics

After all analysis issues are resolved, check the draft for any quantitative metrics or numeric thresholds, such as:
- Performance targets: "less than 15GB/s", "under 100ms latency"
- Size constraints: "below 300KB", "maximum 1MB"
- Count limits: "more than 10 files", "at least 5 retries"
- Percentage goals: "95% coverage", "reduce by 50%"

For each quantitative metric found, use AskUserQuestion to explicitly confirm with the user:
- Is this a **hard requirement** that must be achieved for the implementation to be considered successful?
- Or is this describing an **optimization trend/direction** where improvement toward the target is acceptable even if the exact number is not reached?

Document the user's answer for each metric, as this distinction significantly affects how acceptance criteria should be written in the plan.

---

## Phase 5: Plan Generation

Deeply think and generate the plan content following these rules:

### Plan Structure

```markdown
# <Plan Title>

## Goal Description
<Clear, direct description of what needs to be accomplished>

## Acceptance Criteria

Following TDD philosophy, each criterion includes positive and negative tests for deterministic verification.

- AC-1: <First criterion>
  - Positive Tests (expected to PASS):
    - <Test case that should succeed when criterion is met>
    - <Another success case>
  - Negative Tests (expected to FAIL):
    - <Test case that should fail/be rejected when working correctly>
    - <Another failure/rejection case>
  - AC-1.1: <Sub-criterion if needed>
    - Positive: <...>
    - Negative: <...>
- AC-2: <Second criterion>
  - Positive Tests: <...>
  - Negative Tests: <...>
...

## Path Boundaries

Path boundaries define the acceptable range of implementation quality and choices.

### Upper Bound (Maximum Acceptable Scope)
<Affirmative description of the most comprehensive acceptable implementation>
<This represents completing the goal without over-engineering>
Example: "The implementation includes X, Y, and Z features with full test coverage"

### Lower Bound (Minimum Acceptable Scope)
<Affirmative description of the minimum viable implementation>
<This represents the least effort that still satisfies all acceptance criteria>
Example: "The implementation includes core feature X with basic validation"

### Allowed Choices
<Options that are acceptable for implementation decisions>
- Can use: <technologies, approaches, patterns that are allowed>
- Cannot use: <technologies, approaches, patterns that are prohibited>

> **Note on Deterministic Designs**: If the draft specifies a highly deterministic design with no choices (e.g., "must use JSON format", "must use algorithm X"), then the path boundaries should reflect this narrow constraint. In such cases, upper and lower bounds may converge to the same point, and "Allowed Choices" should explicitly state that the choice is fixed per the draft specification.

## Feasibility Hints and Suggestions

> **Note**: This section is for reference and understanding only. These are conceptual suggestions, not prescriptive requirements.

### Conceptual Approach
<Text description, pseudocode, or diagrams showing ONE possible implementation path>

### Relevant References
<Code paths and concepts that might be useful>
- <path/to/relevant/component> - <brief description>

## Dependencies and Sequence

### Milestones
1. <Milestone 1>: <Description>
   - Phase A: <...>
   - Phase B: <...>
2. <Milestone 2>: <Description>
   - Step 1: <...>
   - Step 2: <...>

<Describe relative dependencies between components, not time estimates>

## Implementation Notes

### Code Style Requirements
- Implementation code and comments must NOT contain plan-specific terminology such as "AC-", "Milestone", "Step", "Phase", or similar workflow markers
- These terms are for plan documentation only, not for the resulting codebase
- Use descriptive, domain-appropriate naming in code instead
```

### Generation Rules

1. **Terminology**: Use Milestone, Phase, Step, Section. Never use Day, Week, Month, Year, or time estimates.
2. **No Line Numbers**: Reference code by path only, never by line ranges.
3. **No Time Estimates**: Do not estimate duration, effort, or code line counts.
4. **Conceptual Not Prescriptive**: Path boundaries and suggestions guide without mandating.
5. **AC Format**: All acceptance criteria must use AC-X or AC-X.Y format.
6. **Clear Dependencies**: Show what depends on what, not when things happen.
7. **TDD-Style Tests**: Each acceptance criterion must include both positive and negative tests.
8. **Affirmative Path Boundaries**: Describe upper and lower bounds using affirmative language.
9. **Respect Deterministic Designs**: If the draft specifies a fixed approach with no choices, reflect this in the plan.
10. **Code Style Constraint**: The plan must explicitly state that plan markers like `AC-`, `Milestone`, `Step`, and `Phase` do not belong in production code/comments.
11. **Draft Completeness Requirement**: Preserve all original draft information and treat clarifications as additive.

---

## Phase 6: Write and Complete

The output file already contains the plan template structure and the original draft content. Now complete the plan through the following steps:

### Step 1: Update Plan Content

Use the **Edit tool** to update the prepared output file:
- Replace template placeholders with actual plan content
- Keep the original draft section intact at the bottom
- Ensure the final file contains both the structured plan and the original draft for reference

### Step 2: Comprehensive Review

After updating, **read the complete plan file** and verify:
- The plan is complete and comprehensive
- All sections are consistent with each other
- The structured plan aligns with the original draft content
- No contradictions exist between different parts of the document

If inconsistencies are found, fix them using the Edit tool.

### Step 3: Language Unification

Check if the updated plan file contains multiple languages.

If multiple languages are detected:
1. Use **AskUserQuestion** to ask the user:
   - whether they want to unify the language
   - which language to use for unification
2. If the user chooses to unify:
   - translate all content to the chosen language
   - ensure the meaning and intent remain unchanged
   - use the Edit tool to apply the translations
3. If the user declines, leave the document as-is

### Step 4: Report Results

Report to the user:
- path to the generated plan
- summary of what was included
- number of acceptance criteria defined
- whether language was unified

---

## Error Handling

If issues arise during plan generation that require user input:
- use AskUserQuestion to clarify
- document user decisions in the plan context

If unable to generate a complete plan:
- explain what information is missing
- suggest how the user can improve the draft
