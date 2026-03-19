---
name: ask-codex
description: Consult Codex as an independent expert. Sends a question or task to `humanize ask-codex` and returns the response.
argument-hint: "[--model MODEL] [--effort EFFORT] [--timeout SECONDS] [question or task]"
allowed-tools: "Bash(humanize ask-codex:*)"
---

# Ask Codex

Send a question or task to Codex and return the response.

## How to Use

Execute the Humanize CLI command with the user's arguments:

```bash
humanize ask-codex $ARGUMENTS
```

## Interpreting Output

- The command writes Codex's response to **stdout** and status information to **stderr**
- Read the stdout output carefully and incorporate Codex's response into your answer
- If the command exits with a non-zero code, report the error to the user

## Error Handling

| Exit Code | Meaning |
|-----------|---------|
| 0 | Success - Codex response is in stdout |
| 1 | Validation or invocation error |
| 124 | Timeout - suggest using `--timeout` with a larger value |
| Other | Underlying Codex process error - report the exit code and any stderr output |

## Notes

- The response is saved to `.humanize/skill/<timestamp>/output.md` for reference
- Default settings are model `gpt-5.4`, effort `xhigh`, and timeout `3600` seconds
