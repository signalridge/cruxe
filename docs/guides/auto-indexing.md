# Auto-Indexing Setup Guide

CodeCompass ships reference templates under `configs/templates/` for:

- `rust`
- `typescript`
- `python`
- `go`
- `monorepo`

Each template includes:

- `.codecompassignore`
- `hooks/post-commit` (runs `codecompass sync`)
- `hooks/pre-push` (runs `codecompass doctor`)

Both hooks are fail-soft: they log errors and exit `0` so git operations are not blocked.

## Install in a Repository

Example for Rust:

```bash
cp configs/templates/rust/.codecompassignore .
cp configs/templates/rust/hooks/post-commit .git/hooks/post-commit
cp configs/templates/rust/hooks/pre-push .git/hooks/pre-push
chmod +x .git/hooks/post-commit .git/hooks/pre-push
```

## Verify Hook Behavior

1. Make a commit.
2. Confirm `codecompass sync` ran.
3. Run `.git/hooks/pre-push` manually once to verify `doctor` behavior.
4. Inspect logs if needed.

Log path:

```text
~/.codecompass/logs/hook.log
```

## IDE Suggestions

- Terminal-based IDEs: keep hooks as the primary trigger.
- GUI-first IDEs: pair hooks with periodic manual `codecompass sync`.
- Multi-repo/monorepo: prefer the `monorepo` template and tune ignore patterns.

## Troubleshooting

### Hook not executing

- Ensure hook file is executable (`chmod +x`).
- Verify it is installed in `.git/hooks/` with exact hook filename.

### Hook logs errors

- Open `~/.codecompass/logs/hook.log`.
- Validate `codecompass` availability: `command -v codecompass`.
- Run `codecompass doctor --path <repo>` manually.

### Index still stale after commit

- Confirm commit actually triggered post-commit in this repo.
- Run `codecompass sync --workspace <repo>` manually.
