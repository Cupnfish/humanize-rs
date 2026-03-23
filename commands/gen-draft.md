---
description: "Capture a planning draft into .humanize/planning"
argument-hint: "[--stdin|--input <path/to/draft.md>] [--title <draft title>]"
allowed-tools:
  - "Bash(humanize gen-draft:*)"
  - "Read"
  - "Glob"
  - "Grep"
  - "AskUserQuestion"
hide-from-slash-command-tool: "true"
---

# Generate Draft

Read and execute below with ultrathink.

## Hard Constraint: No Coding During Draft Capture

This command MUST ONLY capture a planning draft artifact. It MUST NOT implement code, modify repository source files, make commits, open PRs, or start downstream planning stages.

Permitted writes are limited to the internal planning store created by the native CLI under `.humanize/planning/`.

Do NOT call:
- `humanize gen-plan`
- `humanize setup rlcr`
- any write or edit tool against repository source files

This command is only responsible for producing a reusable draft artifact for later `gen-plan` consumption.

## Workflow Overview

1. Determine whether draft content already exists in `$ARGUMENTS`
2. If not, gather or summarize the draft content from the conversation
3. Resolve a short title suitable for handle generation
4. Persist the draft through the native CLI
5. Report the saved draft handle and next-step command

---

## Phase 0: Parse Arguments

Parse `$ARGUMENTS` and determine:
- whether `--stdin` is present
- whether `--input <path>` is present
- whether `--title <title>` is present

Rules:
- `--stdin` and `--input` are mutually exclusive
- if both appear, report the error directly and stop
- do not invent additional CLI flags

---

## Phase 1: Resolve Draft Content Source

There are two supported source modes:

### Mode A: Existing Draft File

If `$ARGUMENTS` already includes `--input <path>`:
- do not rewrite the file
- do not create temporary files
- run the native CLI with the provided path

### Mode B: Conversation-Derived Draft

If `$ARGUMENTS` does not include `--input`:
- derive a concise draft from the current conversation
- preserve the user's actual requirements and intent
- keep open questions visible instead of fabricating certainty
- if the request is too underspecified to produce even a rough draft, ask a short clarification question

When deriving from conversation:
- produce draft markdown only
- include the most important scope, constraints, goals, and non-goals
- prefer a compact rough draft over a polished specification
- avoid adding details that were not stated or strongly implied

If conversation-derived mode is used, pass the content through stdin:

```bash
humanize gen-draft --stdin [--title "<title>"]
```

Do NOT write your own temporary `draft.md` file.

---

## Phase 2: Resolve Title

If `$ARGUMENTS` already includes `--title`, use it unchanged.

Otherwise:
- derive a short, user-readable title from the draft content
- keep it concise and specific
- prefer noun phrases over sentences
- do not include timestamps, ids, or path fragments

If no reasonable title can be derived, use a generic fallback such as `Draft`.

The native CLI will convert the title into a stable handle and disambiguate collisions.

---

## Phase 3: Run Native Draft Capture

Run exactly one native CLI command:

File mode:

```bash
humanize gen-draft --input <path> [--title "<title>"]
```

Conversation mode:

```bash
humanize gen-draft --stdin [--title "<title>"]
```

Handle failures directly:
- missing input file -> report and stop
- empty content -> report and stop
- invalid argument combination -> report and stop

Do not attempt to recover by writing files manually.

---

## Phase 4: Report Result

On success, report:
- draft handle
- thread identifier
- saved draft path
- recommended next step:
  - `humanize gen-plan`
  - or `humanize gen-plan --draft <handle>` when the user should target this draft explicitly

If the draft came from conversation-derived mode, make it clear that the artifact is now stored internally under `.humanize/planning/` and does not need to be committed.
