
**Special Case - .humanize directory detected**:
The `.humanize/` directory is created by `humanize setup rlcr` and should NOT be committed.
Please add it to .gitignore:
```bash
echo '.humanize*' >> .gitignore
git add .gitignore
```
