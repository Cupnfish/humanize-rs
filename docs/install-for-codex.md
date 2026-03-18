# Install Codex CLI

Humanize no longer uses Codex as a host runtime.
Codex is now used only as the independent review backend.

## Prerequisites

Install the Codex CLI and make sure it is available on `PATH`.

```bash
codex --version
```

## Why Codex Is Required

Humanize uses Codex for:

- RLCR implementation-phase review
- RLCR code-review phase
- PR-loop validation
- `humanize ask-codex`

## Verify Humanize Can Reach Codex

```bash
humanize ask-codex "Say hello from Codex."
```

If `codex` is not on `PATH`, Humanize will fail the review or consultation step with an explicit error.
