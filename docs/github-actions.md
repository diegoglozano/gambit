# GitHub Actions

Doctor can annotate invalid PGN directly in pull requests. Add this workflow as
`.github/workflows/gambit-doctor.yml`, change `games/` to the path containing
your PGN corpus, and commit it:

```yaml
name: Validate PGN

on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: read

jobs:
  doctor:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v6
      - name: Install Gambit
        shell: bash
        run: |
          curl --proto '=https' --tlsv1.2 -LsSf \
            https://github.com/diegoglozano/gambit/releases/download/v0.6.1/gambit-installer.sh | sh
          echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
      - name: Validate PGN corpus
        run: gambit doctor --keep-going --format github games/
```

Each diagnostic becomes a native GitHub error annotation attached to its PGN
path, line, and column. Doctor also prints one concise summary and returns its
normal exit status, so invalid chess data fails the job without extra shell
logic.

`--keep-going` reports up to 100 errors per file. Use `--max-errors N` when a
different limit is more useful:

```yaml
- name: Validate PGN corpus
  run: gambit doctor --max-errors 20 --format github games/
```

Use repository-relative input paths so GitHub can link annotations back to the
checked-out files. If another step downloads or generates the corpus, point
Doctor at that step's output directory instead.

The workflow pins Gambit v0.6.1 so its behavior cannot change unexpectedly.
Update the tag deliberately when adopting a newer release, or use `latest` in
the URL if automatic upgrades are preferable.

The output follows GitHub's
[workflow-command annotation syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands#setting-an-error-message).
