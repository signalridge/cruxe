## 1. Queue data model (cruxe-state)

- [ ] 1.1 Add `semantic_enrichment_queue` table with keys (`project_id`, `ref`, `path`, `generation`).
- [ ] 1.2 Add status fields (`pending/running/done/failed`), retry counters, timestamps, and error code.
- [ ] 1.3 Add dequeue/retry indexes and migration tests.

## 2. Enqueue on code updates (cruxe-indexer)

- [ ] 2.1 On changed file commit, enqueue enrichment job with incremented generation.
- [ ] 2.2 Implement latest-wins coalescing for rapid repeated edits.
- [ ] 2.3 Ensure enqueue failure never blocks indexing commit.

## 3. Background worker execution (cruxe-indexer/runtime)

- [ ] 3.1 Implement bounded worker loop (concurrency cap + dequeue cap).
- [ ] 3.2 Execute embedding generation/upsert from queue payload.
- [ ] 3.3 Implement retry/backoff + terminal failure handling.
- [ ] 3.4 Verify idempotent writes under retry/restart.

## 4. Hot-path decoupling (cruxe-cli + sync_incremental)

- [ ] 4.1 Remove synchronous embedding generation from indexing hot path.
- [ ] 4.2 Keep immediate lexical/symbol index commit behavior unchanged.
- [ ] 4.3 Verify indexing p95 improvement on frequent-edit fixture.

## 5. Metadata and protocol wiring (cruxe-query/cruxe-mcp)

- [ ] 5.1 Add metadata fields: `semantic_enrichment_state`, `semantic_backlog_size`, `semantic_lag_hint`, optional `degraded_reason`.
- [ ] 5.2 Expose metadata in search responses and diagnostics.
- [ ] 5.3 Preserve backward compatibility for clients ignoring new fields.

## 6. Verification and gates

- [ ] 6.1 Run `cargo test --workspace`.
- [ ] 6.2 Run `cargo clippy --workspace`.
- [ ] 6.3 Add benchmark: indexing p95 under burst-edit workload (before/after).
- [ ] 6.4 Add benchmark: semantic freshness convergence time + backlog/fallback rates.
- [ ] 6.5 Attach OpenSpec evidence with latency and freshness metrics.

## Dependency order

```
1 (queue model) → 2 (code enqueue) + 3 (worker) → 4 (hot-path decouple) → 5 (metadata) → 7 (retention/cleanup) → 6 (verification)
```

## 7. Queue retention and cleanup

- [ ] 7.1 Implement TTL policy for `done`/superseded rows and triage TTL for `failed` rows.
- [ ] 7.2 Add periodic bounded cleanup job and metrics (`queue_cleanup_deleted`, `queue_cleanup_duration_ms`).
- [ ] 7.3 Add tests for retention correctness and non-blocking cleanup behavior.
