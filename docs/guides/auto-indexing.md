# Auto-Indexing Setup Guide

Cruxe ships reference templates under `configs/templates/` for:

- `rust`
- `typescript`
- `python`
- `go`
- `monorepo`

Each template includes:

- `.cruxeignore`
- `hooks/post-commit` (runs `cruxe sync`)
- `hooks/pre-push` (runs `cruxe doctor`)

Both hooks are fail-soft: they log errors and exit `0` so git operations are not blocked.

## Install in a Repository

Example for Rust:

```bash
cp configs/templates/rust/.cruxeignore .
cp configs/templates/rust/hooks/post-commit .git/hooks/post-commit
cp configs/templates/rust/hooks/pre-push .git/hooks/pre-push
chmod +x .git/hooks/post-commit .git/hooks/pre-push
```

## Verify Hook Behavior

1. Make a commit.
2. Confirm `cruxe sync` ran.
3. Run `.git/hooks/pre-push` manually once to verify `doctor` behavior.
4. Inspect logs if needed.

Log path:

```text
~/.cruxe/logs/hook.log
```

## IDE Suggestions

- Terminal-based IDEs: keep hooks as the primary trigger.
- GUI-first IDEs: pair hooks with periodic manual `cruxe sync`.
- Multi-repo/monorepo: prefer the `monorepo` template and tune ignore patterns.

## Troubleshooting

### Hook not executing

- Ensure hook file is executable (`chmod +x`).
- Verify it is installed in `.git/hooks/` with exact hook filename.

### Hook logs errors

- Open `~/.cruxe/logs/hook.log`.
- Validate `cruxe` availability: `command -v cruxe`.
- Run `cruxe doctor --path <repo>` manually.

### Index still stale after commit

- Confirm commit actually triggered post-commit in this repo.
- Run `cruxe sync --workspace <repo>` manually.
