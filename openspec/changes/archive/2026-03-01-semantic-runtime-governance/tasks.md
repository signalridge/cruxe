## 1. Queue data model (cruxe-state)

- [x] 1.1 Add `semantic_enrichment_queue` table with keys (`project_id`, `ref`, `path`, `generation`).
- [x] 1.2 Add status fields (`pending/running/done/failed`), retry counters, timestamps, and error code.
- [x] 1.3 Add dequeue/retry indexes and migration tests.

## 2. Enqueue on code updates (cruxe-indexer)

- [x] 2.1 On changed file commit, enqueue enrichment job with incremented generation.
- [x] 2.2 Implement latest-wins coalescing for rapid repeated edits.
- [x] 2.3 Ensure enqueue failure never blocks indexing commit.

## 3. Background worker execution (cruxe-indexer/runtime)

- [x] 3.1 Implement bounded worker loop (concurrency cap + dequeue cap).
- [x] 3.2 Execute embedding generation/upsert from queue payload.
- [x] 3.3 Implement retry/backoff + terminal failure handling.
- [x] 3.4 Verify idempotent writes under retry/restart.

## 4. Hot-path decoupling (cruxe-cli + sync_incremental)

- [x] 4.1 Remove synchronous embedding generation from indexing hot path.
- [x] 4.2 Keep immediate lexical/symbol index commit behavior unchanged.
- [x] 4.3 Verify indexing p95 improvement on frequent-edit fixture.

## 5. Metadata and protocol wiring (cruxe-query/cruxe-mcp)

- [x] 5.1 Add metadata fields: `semantic_enrichment_state`, `semantic_backlog_size`, `semantic_lag_hint`, optional `degraded_reason`.
- [x] 5.2 Expose metadata in search responses and diagnostics.
- [x] 5.3 Preserve backward compatibility for clients ignoring new fields.

## 6. Verification and gates

- [x] 6.1 Run `cargo test --workspace`.
- [x] 6.2 Run `cargo clippy --workspace`.
- [x] 6.3 Add benchmark: indexing p95 under burst-edit workload (before/after).
- [x] 6.4 Add benchmark: semantic freshness convergence time + backlog/fallback rates.
- [x] 6.5 Attach OpenSpec evidence with latency and freshness metrics.

## Dependency order

```
1 (queue model) → 2 (code enqueue) + 3 (worker) → 4 (hot-path decouple) → 5 (metadata) → 7 (retention/cleanup) → 6 (verification)
```

## 7. Queue retention and cleanup

- [x] 7.1 Implement TTL policy for `done`/superseded rows and triage TTL for `failed` rows.
- [x] 7.2 Add periodic bounded cleanup job and metrics (`queue_cleanup_deleted`, `queue_cleanup_duration_ms`).
- [x] 7.3 Add tests for retention correctness and non-blocking cleanup behavior.
