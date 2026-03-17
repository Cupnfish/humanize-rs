---
name: ask-codex
description: Consult Codex as an independent expert. Sends a question or task to `humanize ask-codex` and returns the response.
argument-hint: "[--model MODEL] [--effort EFFORT] [--timeout SECONDS] [question or task]"
---

# Ask Codex

Send a question or task to the native Rust `humanize` binary and return the response.

## Runtime Root

The installer hydrates this skill with an absolute runtime root path:

```bash
{{HUMANIZE_RUNTIME_ROOT}}
```

The runtime root provides prompt templates and related assets, while the `humanize` executable is expected to be available on `PATH`.

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize
```

## How to Use

Execute:

```bash
CLAUDE_PLUGIN_ROOT="{{HUMANIZE_RUNTIME_ROOT}}" humanize ask-codex $ARGUMENTS
```

## Interpreting Output

- The command prints Codex's response to `stdout`
- Validation or process errors are reported on `stderr`
- If the command exits non-zero, report the failure instead of pretending success

## Error Handling

| Exit Code | Meaning |
|-----------|---------|
| 0 | Success |
| 1 | Validation or runtime error |
| 124 | Timeout |
| Other | Codex process error |

## Notes

- Responses are saved under `.humanize/skill/<timestamp>/`
- Default configuration is `gpt-5.4`, `xhigh`, timeout `3600`
