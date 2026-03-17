# Install for Kimi

Install hydrated skills for the Kimi runtime:

```bash
cargo run -- install-skills --target kimi
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

The installer writes the runtime binary to:

```text
<skills-dir>/humanize/bin/humanize
```

and installed skills invoke that binary directly.
