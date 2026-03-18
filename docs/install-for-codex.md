# Install for Codex

Install hydrated skills for the Codex runtime:

```bash
humanize install --target codex
```

Default install location:

```text
${CODEX_HOME:-~/.codex}/skills/
```

Installed skills:

- `ask-codex`
- `humanize`
- `humanize-gen-plan`
- `humanize-rlcr`

The installer does **not** place a binary under the skills directory.
Installed skills expect:

- `humanize` on `PATH`

The installer writes only skill definitions into the target skills directory.
