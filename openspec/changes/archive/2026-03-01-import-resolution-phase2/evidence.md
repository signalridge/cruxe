# Evidence — import-resolution-phase2 (2026-03-01)

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace
cargo test -p cruxe-indexer import_extract::tests::resolve_imports_with_stats_emits_resolution_rates
```

Artifacts:

- `target/openspec-evidence/test-workspace.log`
- `target/openspec-evidence/clippy-workspace.log`
- `target/openspec-evidence/test-import-resolution.log`
- `target/openspec-evidence/index-main-off.log`
- `target/openspec-evidence/index-main-hybrid-local.log`
- `target/openspec-evidence/import-latency-and-resolution-summary.json`

## Baseline quality tuning evidence (resolver counters)

Aggregate resolver stats parsed from index logs (`records=185` each profile):

| Profile | attempts | resolved | unresolved | resolved rate | unresolved rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| `off` | 1598 | 941 | 657 | 0.5889 | 0.4111 |
| `hybrid-local` | 1598 | 941 | 657 | 0.5889 | 0.4111 |

Provider distribution in logs is deterministic (`generic_heuristic`).

## Index latency non-regression check

| Profile | Full-index duration (s) |
| --- | ---: |
| `off` | 3.5 |
| `hybrid-local` | 4.2 |

Delta: `+0.7s` on full index run for this workspace snapshot.
