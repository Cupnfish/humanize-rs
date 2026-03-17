# Install for Codex

Install hydrated skills for the Codex runtime:

```bash
cargo run -- install-skills --target codex
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

The installer also places the runtime binary under:

```text
<skills-dir>/humanize/bin/humanize
```

and rewrites installed `SKILL.md` files to call that binary directly.
