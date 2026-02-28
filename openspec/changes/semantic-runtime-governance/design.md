## Context

Cruxe already has incremental indexing primitives, but expensive semantic work can inflate update latency when repositories churn frequently. The architecture must guarantee:

1. immediate baseline searchability after code updates,
2. eventual semantic freshness via asynchronous enrichment,
3. no hard failure coupling from semantic runtime to indexing completion.

## Goals / Non-Goals

**Goals**
1. Split indexing into synchronous hot path and async enrichment path.
2. Add deterministic queue semantics with latest-wins coalescing.
3. Expose backlog/degraded freshness metadata.
4. Keep all semantic/runtime failures fail-soft.

**Non-Goals**
1. Distributed worker orchestration.
2. Exactly-once guarantees (idempotent at-least-once is sufficient).
3. Running heavyweight enrichment work in indexing hot path.

## Decisions

### D1. Hot path vs async enrichment split

Hot path (synchronous):
- parse/extract symbols,
- write Tantivy symbols/snippets/files,
- persist core manifest/relations for immediate retrieval,
- commit and return.

Async path:
- embedding generation/upsert,
- semantic refresh tasks that are not required for immediate retrieval.

### D2. Queue model with latest-wins coalescing

Queue key: `(project_id, ref, path)` with `generation`.

Fields:
- `status` (`pending`, `running`, `done`, `failed`),
- retry/backoff metadata,
- last error code.

If a newer generation arrives, older work for same key is superseded.

### D3. Worker lifecycle and budgets

Bounded worker controls:
- max concurrency,
- per-job timeout,
- dequeue cap per cycle,
- exponential/backoff retry policy.

High backlog transitions runtime state to `backlog`/`degraded`, while lexical retrieval remains available.

### D4. Retrieval semantics under lag

Response metadata includes:
- `semantic_enrichment_state` (`ready | backlog | degraded`),
- `semantic_backlog_size`,
- `semantic_lag_hint`,
- optional `degraded_reason`.

Semantic ranking may be stale/degraded; lexical fallback remains authoritative.

### D5. Idempotent writes

Semantic upserts stay idempotent (project/ref/symbol/snippet-hash/model-version keyed), enabling safe retries and restart recovery.

### D6. Queue retention and cleanup

Queue rows are lifecycle-managed to prevent unbounded growth:

- keep `done` rows for short diagnostics window (for example 24-72h),
- keep `failed` rows longer for triage window (for example 7d),
- compact superseded generations aggressively,
- run periodic cleanup job with bounded batch deletes.

Cleanup MUST be non-blocking to query/indexing hot paths.

## Risks / Trade-offs

- **Risk: eventual consistency may temporarily show stale semantic ranking.**
  - Mitigation: explicit freshness metadata + lexical fallback.

- **Risk: queue growth during burst edits.**
  - Mitigation: latest-wins coalescing + bounded workers + retry backoff.

- **Trade-off: increased runtime complexity.**
  - Accepted to protect indexing p95 and interactive responsiveness.

## Migration Plan

1. Add queue schema + enqueue in shadow mode.
2. Enable workers at low concurrency with observability only.
3. Remove synchronous embedding from hot path.
4. Enable backlog/degraded metadata and tune thresholds.

Rollback:
- disable worker consumption and return to lexical-first degraded semantic behavior.
