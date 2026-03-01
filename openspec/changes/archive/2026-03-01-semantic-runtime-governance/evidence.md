# Evidence — semantic-runtime-governance (2026-03-01)

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace
cargo test -p cruxe-indexer sync_incremental::tests::t274_incremental_sync_ten_file_smoke_under_five_seconds
cargo test -p cruxe-indexer sync_incremental::tests::t294_overlay_bootstrap_fifty_file_smoke_under_fifteen_seconds
cargo test -p cruxe-state semantic_queue::tests::runtime_state_reports_backlog_and_degraded
```

Artifacts:

- `target/openspec-evidence/test-workspace.log`
- `target/openspec-evidence/clippy-workspace.log`
- `target/openspec-evidence/test-semantic-runtime-frequent-edit.log`
- `target/openspec-evidence/test-semantic-runtime-burst-edit.log`
- `target/openspec-evidence/test-semantic-runtime-state.log`
- `target/openspec-evidence/semantic-runtime-benchmark-summary.json`
- `target/openspec-evidence/semantic-local-diversity-on.json`

## Frequent-edit / burst-edit latency checks

From targeted incremental-sync smoke tests:

| Workload check | Result |
| --- | ---: |
| 10-file frequent-edit smoke | 1.08s (pass) |
| 50-file burst-edit bootstrap smoke | 1.07s (pass) |

## Freshness / backlog / fallback signals

- `semantic_queue::tests::runtime_state_reports_backlog_and_degraded` passes (backlog/degraded state transitions validated).
- Search metadata run (`semantic-local-diversity-on.json`) reports:
  - `degraded_query_rate = 0.0`
  - `semantic_budget_exhaustion_rate = 1.0`
  - `rerank_fallback_rate = 0.0`

These artifacts provide the governance envelope for latency + runtime-state monitoring.
