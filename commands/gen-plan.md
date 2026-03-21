---
description: "Generate implementation plan from draft document"
argument-hint: "--input <path/to/draft.md> --output <path/to/plan.md> [--auto-start-rlcr-if-converged] [--discussion|--direct]"
allowed-tools:
  - "Bash(humanize gen-plan:*)"
  - "Bash(humanize ask-codex:*)"
  - "Bash(humanize setup rlcr:*)"
  - "Bash(humanize config merged:*)"
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

Read and execute below with ultrathink.

## Hard Constraint: No Coding During Plan Generation

This command MUST ONLY generate a plan document during the planning phases. It MUST NOT implement tasks, modify repository source code, or make commits or PRs while producing the plan.

Permitted writes before any optional auto-start are limited to:
- the plan output file (`--output`)
- an optional translated language variant when configured

If `--auto-start-rlcr-if-converged` is enabled, the command MAY immediately set up the RLCR loop with `humanize setup rlcr <output-plan-path>`, but only when the plan is converged, `GEN_PLAN_MODE=discussion`, and there are no pending user decisions. All coding happens after plan generation, not during it.

## Workflow Overview

> **Sequential Execution Constraint**: Execute all phases strictly in order. Do NOT parallelize tool calls across different phases. Each phase must complete before the next one begins.

1. **Execution Mode Setup**: Parse optional behaviors from command arguments
2. **Load Project Config**: Resolve merged Humanize config defaults for `alternative_plan_language` and `gen_plan_mode`
3. **IO Validation and Scaffold Preparation**: Validate input and output paths and create the initial output scaffold
4. **Relevance Check**: Verify draft is relevant to the repository
5. **Codex First-Pass Analysis**: Use one planning Codex before Claude synthesizes plan details
6. **Claude Candidate Plan (v1)**: Build an initial plan from draft plus Codex findings
7. **Iterative Convergence Loop**: Claude and a second Codex iteratively challenge and refine plan reasonability
8. **Issue and Disagreement Resolution**: Resolve unresolved issues, metrics, and opposite opinions
9. **Final Plan Generation**: Generate the converged structured `plan.md`
10. **Write and Complete**: Update the output file, optionally write translated variant, optionally auto-start implementation, and report results

---

## Phase 0: Execution Mode Setup

Parse `$ARGUMENTS` and set:
- `AUTO_START_RLCR_IF_CONVERGED=true` if `--auto-start-rlcr-if-converged` is present
- `AUTO_START_RLCR_IF_CONVERGED=false` otherwise
- `GEN_PLAN_MODE_DISCUSSION=true` if `--discussion` is present
- `GEN_PLAN_MODE_DIRECT=true` if `--direct` is present
- If both `--discussion` and `--direct` are present simultaneously, report error `Cannot use --discussion and --direct together` and stop

`AUTO_START_RLCR_IF_CONVERGED=true` allows skipping manual review and immediately running `humanize setup rlcr <output-plan-path>`, but only when `GEN_PLAN_MODE=discussion`, plan convergence is achieved, and no pending user decisions remain. In `direct` mode this condition is never satisfied.

---

## Phase 0.5: Load Project Config

After setting execution mode flags, resolve configuration using the native CLI helper:

```bash
humanize config merged --json --with-meta
```

Treat non-zero exit as a configuration error and stop.

The command returns JSON with:
- `merged`: merged config object
- `explicit_user_keys`: top-level keys explicitly present in user config
- `explicit_project_keys`: top-level keys explicitly present in project config

### Values to Extract

Read these values from the returned JSON:
- `merged.alternative_plan_language`
- `merged.gen_plan_mode`
- `merged.chinese_plan` (legacy fallback only)
- whether `alternative_plan_language` appears in either `explicit_user_keys` or `explicit_project_keys`

### Alternative Language Resolution

Resolve the effective language value with this priority:
1. If `alternative_plan_language` is explicitly set in user or project config, use that value even if it is empty
2. Otherwise, if `merged.chinese_plan=true`, treat the effective language as `Chinese`
3. Otherwise, treat translation as disabled

Normalize the effective language with this mapping table. Matching is case-insensitive and trims whitespace.

| Language   | Code | Suffix |
|------------|------|--------|
| Chinese    | zh   | `_zh`  |
| Korean     | ko   | `_ko`  |
| Japanese   | ja   | `_ja`  |
| Spanish    | es   | `_es`  |
| French     | fr   | `_fr`  |
| German     | de   | `_de`  |
| Portuguese | pt   | `_pt`  |
| Russian    | ru   | `_ru`  |
| Arabic     | ar   | `_ar`  |

Rules:
- empty or absent value: disable translation
- `English` or `en`: disable translation
- supported language name or code: set `ALT_PLAN_LANGUAGE` and `ALT_PLAN_LANG_CODE`
- unsupported value: disable translation and log a warning in the final notes

### Gen-Plan Mode Resolution

Resolve `GEN_PLAN_MODE` with this priority:
1. CLI flags from Phase 0
2. `merged.gen_plan_mode` when it is `discussion` or `direct`
3. default to `discussion`

If config provides an invalid `gen_plan_mode` value, treat it as absent and note the warning in the final report.

---

## Phase 1: IO Validation and Scaffold Preparation

Run the native helper:

```bash
humanize gen-plan --prepare-only $ARGUMENTS
```

Handle failures directly:
- input file missing or empty -> report and stop
- output directory missing -> report and stop
- output file already exists -> report and stop
- plan template missing -> report plugin/runtime configuration error and stop

On success, the output file already exists and already contains:
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
3. If result is `NOT_RELEVANT`:
   - report that the draft does not appear to be related to this repository
   - show the reason
   - stop
4. If result is `RELEVANT`, continue to Phase 3

---

## Phase 3: Codex First-Pass Analysis

After relevance check, invoke Codex BEFORE Claude plan synthesis.

Run:

```bash
humanize ask-codex "<structured prompt>"
```

The structured prompt MUST include:
- repository context
- raw draft content
- explicit request to critique assumptions, identify missing requirements, and propose stronger plan directions

Require Codex output to follow this format:
- `CORE_RISKS:`
- `MISSING_REQUIREMENTS:`
- `TECHNICAL_GAPS:`
- `ALTERNATIVE_DIRECTIONS:`
- `QUESTIONS_FOR_USER:`
- `CANDIDATE_CRITERIA:`

Preserve this output as **Codex Analysis v1** and feed it into Claude planning.

If `humanize ask-codex` fails, use AskUserQuestion and let the user choose:
- retry with updated Codex settings or environment
- continue with Claude-only planning and explicitly note reduced cross-review confidence

---

## Phase 4: Claude Candidate Plan (v1)

Use draft content plus Codex Analysis v1 to produce an initial candidate plan and issue map.

Deeply analyze the draft for potential issues. Use Explore agents to investigate the codebase.

Alongside candidate plan v1, prepare a concise implementation summary covering:
- scope
- system boundaries
- dependencies
- known risks

### Analysis Dimensions

1. **Clarity**
   - are objectives well-defined
   - is the scope clear
   - are terms and concepts unambiguous
2. **Consistency**
   - are requirements internally consistent
   - do different sections align
3. **Completeness**
   - investigate parts of the codebase the draft might affect
   - identify dependencies, side effects, or related components not mentioned
   - check for overlooked edge cases
4. **Functionality**
   - would the proposed approach actually work
   - are there technical limitations not addressed
   - could the design negatively impact existing functionality

### Exploration Strategy

Use the Task tool with `subagent_type: "Explore"` to investigate:
- components mentioned in the draft
- related files and directories
- existing patterns and conventions
- dependencies and integrations

---

## Phase 5: Iterative Convergence Loop (Claude <-> Second Codex)

If `GEN_PLAN_MODE=direct`, skip this entire phase. The plan proceeds directly from candidate plan v1 to Phase 6. Since no second-pass review occurred, set `PLAN_CONVERGENCE_STATUS=partially_converged` and `HUMAN_REVIEW_REQUIRED=true`.

If `GEN_PLAN_MODE=discussion`, run iterative challenge and refine rounds with a SECOND Codex pass.

### Convergence Round Steps

1. Run:
   ```bash
   humanize ask-codex "<review current candidate plan>"
   ```
2. The review prompt MUST include:
   - the current candidate plan
   - prior disagreements
   - unresolved items
3. Require output format:
   - `AGREE:`
   - `DISAGREE:`
   - `REQUIRED_CHANGES:`
   - `OPTIONAL_IMPROVEMENTS:`
   - `UNRESOLVED:`
4. Claude revises the candidate plan to address `REQUIRED_CHANGES`
5. Claude documents accepted and rejected suggestions with rationale
6. Maintain a convergence matrix with:
   - topic
   - Claude position
   - second Codex position
   - resolution status (`resolved`, `needs_user_decision`, `deferred`)
   - round-to-round delta

### Loop Termination Rules

Repeat convergence rounds until one of the following is true:
- no `REQUIRED_CHANGES` remain and no high-impact `DISAGREE` remains
- two consecutive rounds produce no material plan changes
- maximum 3 rounds reached

If maximum rounds are reached with unresolved opposite opinions, carry them to user decision phase explicitly.

Set convergence state explicitly:
- `PLAN_CONVERGENCE_STATUS=converged` when convergence conditions are met
- `PLAN_CONVERGENCE_STATUS=partially_converged` otherwise

---

## Phase 6: Issue and Disagreement Resolution

> **Critical**: The draft document contains the most valuable human input. During issue resolution, NEVER discard or override any original draft content. All clarifications should be treated as incremental additions that supplement the draft, not replacements.

### Step 1: Manual Review Gate

Decide if manual review can be skipped:
- if `GEN_PLAN_MODE=direct`, set `HUMAN_REVIEW_REQUIRED=true`
- else if `AUTO_START_RLCR_IF_CONVERGED=true` and `PLAN_CONVERGENCE_STATUS=converged`, set `HUMAN_REVIEW_REQUIRED=false`
- otherwise set `HUMAN_REVIEW_REQUIRED=true`

If `HUMAN_REVIEW_REQUIRED=false`, skip Step 2 to Step 4 and continue directly to Phase 7.

### Step 1.5: Consolidate Pending User Decisions

Before proceeding, consolidate all unresolved user-facing questions into the plan's `## Pending User Decisions` section:
- items from `QUESTIONS_FOR_USER` in Codex Analysis v1
- items with status `needs_user_decision` from the convergence matrix

Deduplicate merged topics. Remove only items clearly resolved during refinement. Every remaining item must be listed as `DEC-N` with `Decision Status: PENDING`.

### Step 2: Resolve Analysis Issues

If any issues are found during Codex-first analysis, Claude analysis, or convergence, use AskUserQuestion to clarify with the user.

For each issue:
- explain what the issue is
- explain why it matters
- provide options for resolution when applicable

Continue until all significant issues are resolved or explicitly acknowledged.

### Step 3: Confirm Quantitative Metrics

For each quantitative metric or numeric threshold, use AskUserQuestion to confirm whether it is:
- a **hard requirement**
- or an **optimization trend or direction**

Document the user's answer and carry it into acceptance criteria.

### Step 4: Resolve Unresolved Claude/Codex Disagreements

For every item marked `needs_user_decision`, explicitly ask the user to decide.

After the user answers:
- update the convergence matrix
- update `## Pending User Decisions`
- resolve or annotate any affected sections of the candidate plan

---

## Phase 7: Final Plan Generation

Generate the final `plan.md` using:
- the original draft
- repository context
- Codex Analysis v1
- the converged candidate plan
- all user clarifications
- all metric interpretations

### Plan Requirements

The final plan MUST:
- preserve all meaningful information from the draft
- treat clarifications as additive, not replacements
- follow the plan template structure
- include acceptance criteria using `AC-X` or `AC-X.Y`
- include positive and negative tests for each acceptance criterion
- include path boundaries
- include dependencies and milestones
- include implementation notes telling engineers not to use plan markers such as `AC-`, `Milestone`, `Step`, or `Phase` inside production code or comments
- include task routing tags when a task breakdown is present
- include a `## Claude-Codex Deliberation` section summarizing agreements, resolved disagreements, and convergence status
- include a `## Pending User Decisions` section reflecting the final resolved or unresolved state

---

## Phase 8: Write and Complete

### Step 1: Update the Prepared Output File

Use the Edit tool to replace the template placeholders inside the already prepared output file:
- keep the original draft section intact at the bottom
- review the full file for consistency
- ensure the final file is a superset of original draft plus clarifications

### Step 2: Optional Language Variant

If `ALT_PLAN_LANGUAGE` is enabled, write a translated language variant beside the main plan file using the `_<code>` suffix before the extension.

The translated variant must preserve:
- identifiers such as `AC-*`
- file paths
- command names
- code identifiers

### Step 3: Auto-Start Gate

If `AUTO_START_RLCR_IF_CONVERGED=true`, only auto-start when ALL of the following are true:
- `GEN_PLAN_MODE=discussion`
- `PLAN_CONVERGENCE_STATUS=converged`
- no `PENDING` decisions remain

If those conditions are satisfied, run:

```bash
humanize setup rlcr <output-plan-path>
```

Report that the RLCR loop has been set up.

If conditions are not satisfied, report why auto-start was skipped.

### Step 4: Final Report

Report:
- plan output path
- convergence status
- whether translation variant was generated
- whether auto-start was triggered
- any remaining unresolved user decisions
