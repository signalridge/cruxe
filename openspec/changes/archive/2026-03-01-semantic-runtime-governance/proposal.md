## Why

Cruxe's indexing path must remain responsive under frequent code updates. Expensive semantic work (especially embedding generation) should not run inline with indexing commit.

We need deterministic **asynchronous semantic enrichment**:

- fast searchable baseline committed immediately,
- heavier semantic enrichment executed in background,
- transparent runtime state and fail-soft behavior.

## What Changes

1. Add async enrichment pipeline with hot-path/cold-path split:
   - Hot path: lexical/symbol/file index commit.
   - Background path: semantic embeddings.
2. Add enrichment queue contract with coalescing and latest-wins semantics.
3. Add runtime lifecycle states (`idle`, `backlog`, `draining`, `degraded`) with health metadata.
4. Add strict fail-soft guarantees: enrichment failures never block indexing or search serving.

## Capabilities

### New Capabilities
- `async-semantic-enrichment`: background semantic enrichment pipeline with deterministic queue semantics.

### Modified Capabilities
- `semantic-retrieval-quality`: semantic-degraded semantics now include async backlog/degraded state reasons.

## Impact

- Affected crates: `cruxe-indexer`, `cruxe-query`, `cruxe-state`, `cruxe-core`, `cruxe-mcp`.
- API impact: additive metadata fields for enrichment backlog/state.
- Product impact: lower indexing p95 under frequent updates, with eventual semantic convergence.
