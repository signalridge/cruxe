# Research: Embedded Vector Backend + Local Model Decisions

**Spec**: 008-semantic-hybrid | **Date**: 2026-02-23

## Decision 1: Embedded Vector Store Selection

### Requirements

- Must be embeddable (no external service, no Docker, no server process)
- Must be compatible with Rust (native or FFI)
- Must support CRUD operations for vector records
- Must support approximate nearest neighbor (ANN) search
- Must handle up to 50,000 vectors efficiently

### Options Evaluated

| Option | Embedding | Rust Support | ANN Algorithm | License |
|--------|-----------|-------------|---------------|---------|
| Local segment/table (default) | Yes (embedded) | Rust-native | backend-defined | project-native |
| LanceDB adapter (optional) | Yes (embedded) | Rust-native | IVF-PQ, HNSW | Apache 2.0 |
| Qdrant (embedded mode) | Yes (embedded) | Rust-native | HNSW | Apache 2.0 |
| SQLite + sqlite-vss | Yes (extension) | Via rusqlite | IVF | MIT |
| faiss (via FFI) | C++ library | FFI bindings | IVF, HNSW, PQ | MIT |

### Decision: Default local embedded store + optional adapters

**Rationale**:
- Keeps zero-external-service baseline and minimizes dependency lock-in
- Preserves strict control over schema/model-version migration policy
- Allows optional adapter enablement (for example LanceDB) without making it mandatory
- Supports incremental update semantics needed by `sync_repo`

**Trade-offs**:
- More in-house implementation responsibility for ANN/runtime tuning
- Adapter layer adds interface complexity, but keeps rollout flexibility

### Rejected Alternatives

- **Qdrant embedded**: Good option but heavier runtime footprint for default path
- **sqlite-vss**: Extension-based approach is fragile for distribution; limited ANN algorithms
- **faiss**: C++ FFI adds build complexity and cross-platform risk

---

## Decision 2: Embedding Model Strategy (Profile-Based)

### Requirements

- Must produce meaningful embeddings for code snippets (not just natural language)
- Must be runnable locally without external API (for offline/air-gapped use)
- External API option for users who prefer higher quality
- Embedding dimensions should be reasonable (384-768) for storage efficiency
- Inference time should be < 10ms per snippet on modern hardware

### Options Evaluated

| Model Family | Dimensions | Code-Aware | Local | Profile Fit |
|--------------|-----------|------------|-------|-------------|
| `NomicEmbedTextV15Q` | 768 | Good | Yes (fastembed) | `fast_local` |
| `BGESmallENV15Q` | 384 | Good | Yes (fastembed) | `fast_local` |
| `BGEBaseENV15Q` | 768 | Better | Yes (fastembed) | `code_quality` |
| `JinaEmbeddingsV2BaseCode` | 768 | Strong | Yes (fastembed) | `code_quality` |
| `BGELargeENV15` / `GTELargeENV15` | 1024 | Stronger | Yes (fastembed) | `high_quality` |
| Voyage Code 2 | 1024 | Strong | No (API) | `external` |

### Decision: Two default profiles, `fast_local` first

**Profiles**:

- `fast_local` (default): quantized local models (`NomicEmbedTextV15Q` or `BGESmallENV15Q`)
- `code_quality` (optional): code-aware local models (`BGEBaseENV15Q`, `JinaEmbeddingsV2BaseCode`)
- `high_quality` (optional): larger local models (`BGELargeENV15`, `GTELargeENV15`, `SnowflakeArcticEmbedL`)

**Rationale**:
- Keeps default installation/runtime lightweight
- Preserves a clear upgrade path for quality-sensitive users
- Avoids forcing all users into high-dimension/high-latency embeddings

**Configuration approach**:
```toml
[semantic]
mode = "off"
ratio = 0.3

[semantic.embedding]
profile = "fast_local"    # "fast_local" | "code_quality" | "high_quality" | "external"
provider = "local"        # "local" | "openai" | "voyage"
model = "NomicEmbedTextV15Q"
model_version = "fastembed-1"
dimensions = 768
# api_key read from CRUXE_EMBEDDING_API_KEY env var
```

**Trade-offs**:
- `fast_local` has lower semantic quality than larger code-specialized models
- `code_quality`/`high_quality` profiles increase index size and latency
- Profile split keeps defaults practical while preserving quality headroom

---

## Decision 3: Rerank Provider Selection

### Requirements

- Must support code-aware reranking
- Must be accessible via API
- Must have reasonable pricing for developer use
- Must fail-soft (provider unavailable = fallback to local)

### Options Evaluated

| Provider | Code Support | Pricing | Latency (p95) |
|----------|-------------|---------|---------------|
| Cohere Rerank v3 | Good | $1/1000 queries | ~200ms |
| Voyage Rerank 2 | Excellent (code-focused) | $0.05/1000 queries | ~150ms |
| Jina Reranker v2 | Good | Free tier available | ~300ms |

### Decision: Implement Cohere Rerank v3 first, Voyage as second option

**Rationale**:
- Cohere has wider adoption and more stable API
- Voyage has better code-specific performance but is newer
- Trait abstraction means adding providers is low-effort
- Both are optional; local rule-based reranker is always available

**Trait interface**:
```rust
#[async_trait]
pub trait Rerank: Send + Sync {
    async fn rerank(
        &self,
        query: &str,
        documents: &[RerankDocument],
        top_n: usize,
    ) -> Result<Vec<RerankResult>, RerankError>;
}
```

---

## Decision 4: Hybrid Score Blending Strategy

### Approach: Reciprocal Rank Fusion (RRF) with weighted inputs

```
hybrid_score = (1 - ratio) * lexical_rrf_rank + ratio * semantic_rrf_rank
```

Where:
- `ratio` = `semantic_ratio` from config or per-request
- `lexical_rrf_rank` = 1 / (k + lexical_rank), k = 60 (standard RRF constant)
- `semantic_rrf_rank` = 1 / (k + semantic_rank)

**Rationale**: RRF is robust to score distribution differences between lexical and
semantic systems. Weighted RRF lets users control the blend smoothly from 0.0 (pure
lexical) to 1.0 (pure semantic).

**Alternative considered**: Score normalization + linear interpolation. Rejected because
lexical (BM25) and semantic (cosine similarity) scores have very different distributions,
making normalization fragile.

---

## Decision 5: Vector Identity and Model Versioning

### Decision

Use stable identity keys and explicit model version partitioning:

`(project_id, ref, symbol_stable_id, snippet_hash, embedding_model_version)`

### Rationale

- Prevents false duplicates when line ranges move
- Keeps vector lifecycle deterministic across ref changes and incremental sync
- Avoids invalid cross-model similarity comparisons

### Consequences

- Model/version change requires background re-embed or lazy regeneration
- Query path must filter vectors by matching model version

---

## Decision 6: Semantic Trigger Policy and Confidence Model

### Decision

1. `semantic_ratio` is a cap, not a forced fixed weight.
2. Skip semantic branch when lexical confidence is already high
   (`lexical_short_circuit_threshold`).
3. Low-confidence decision uses composite signals:
   - top score
   - top1-top2 margin
   - lexical/semantic agreement

### Rationale

- Reduces avoidable latency/cost for easy queries
- Improves confidence stability across heterogeneous score distributions
- Produces better agent follow-up suggestions than single-threshold scoring

---

## Decision 7: External Provider Safety Policy

### Decision

External embedding/rerank calls require both:

- `external_provider_enabled = true`
- `allow_code_payload_to_external = true`

Defaults remain `false`.

### Rationale

- Secure-by-default behavior for local/private codebases
- Clear operator intent required before any outbound code payload
- Aligns with existing guardrail model in `specs/meta/design.md`

---

## Open Questions

1. **Model download strategy**: Bundle vs first-use download for `fast_local` model.
   **Recommendation**: first-use download + checksum verification + local cache.

2. **Embedding batch size**: Optimal batch size for each model profile.
   **Recommendation**: default `32`, benchmark by profile/hardware.

3. **ANN maintenance policy**: Incremental update frequency vs periodic rebuild cadence.
   **Recommendation**: incremental on sync, full rebuild on `index --force`.

4. **Profile auto-selection**: Whether to auto-switch profile based on corpus size.
   **Recommendation**: keep explicit operator control initially; revisit with telemetry.
