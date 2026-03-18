# Install for Kimi

Install hydrated skills for the Kimi runtime:

```bash
humanize install --target kimi
```

Default install location:

```text
~/.config/agents/skills/
```

Installed skills:

- `ask-codex`
- `humanize`
- `humanize-gen-plan`
- `humanize-rlcr`

The installer does **not** write a binary into the skills directory.
Installed skills expect:

- `humanize` on `PATH`

The installer writes only skill definitions into the target skills directory.
