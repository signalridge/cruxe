## ADDED Requirements

### Requirement: Semantic runtime quality contract extends (not duplicates) symbol-contract parity
`semantic-retrieval-quality-contract` MUST extend runtime quality semantics on top of `refactor-multilang-symbol-contract` task 6.8, and MUST NOT re-specify the same enrichment implementation as a separate mechanism.

Scope boundary:
- Upstream (`refactor...` task 6.8): semantic hit metadata enrichment parity before ranking.
- This change: degraded-state signaling, budget bounding/observability, scale-tier behavior, and benchmark acceptance.

#### Scenario: Runtime contract is layered on existing enrichment parity
- **WHEN** semantic-only hits are enriched by stable-id lookup from the prior change
- **THEN** this change MUST only add runtime quality metadata/controls
- **AND** MUST NOT introduce a second independent enrichment pipeline

### Requirement: Normalized semantic degraded-state signaling
Search responses MUST expose a normalized semantic degradation signal that is deterministic and additive.

State fields:
- `semantic_triggered` (existing)
- `semantic_skipped_reason` (existing)
- `semantic_fallback` (existing)
- `semantic_degraded` (**new**, additive)

Normalization rule:
- `semantic_degraded = semantic_fallback`

Deterministic interpretation:
- semantic active: `semantic_triggered=true`, `semantic_fallback=false`, `semantic_degraded=false`
- semantic intentionally skipped (policy/intent/mode): `semantic_fallback=false`, `semantic_degraded=false`
- semantic backend failure fallback: `semantic_fallback=true`, `semantic_degraded=true`

#### Scenario: Backend failure sets degraded=true and keeps lexical results
- **WHEN** embedding generation or vector retrieval fails during semantic execution
- **THEN** the response MUST return lexical results (no hard error)
- **AND** MUST set `semantic_fallback=true`, `semantic_degraded=true`
- **AND** MUST set `semantic_skipped_reason` to a deterministic failure code

#### Scenario: Intentional skip is not reported as degraded
- **WHEN** semantic path is skipped for non-failure reasons (`semantic_disabled`, `intent_not_nl`, `mode_rerank_only`, `lexical_high_confidence`, `project_scope_unresolved`, `semantic_requires_state_connection`)
- **THEN** `semantic_fallback=false` and `semantic_degraded=false`

### Requirement: Hybrid candidate budget bounds and observability
Hybrid semantic execution MUST enforce bounded candidate budgets and expose effective runtime values in metadata.

Budget derivation inputs:
- `search.semantic.semantic_limit_multiplier`
- `search.semantic.lexical_fanout_multiplier`
- `search.semantic.semantic_fanout_multiplier`

Effective bounds:
- floors: `semantic_limit >= 20`, `lexical_fanout >= 40`, `semantic_fanout >= 30`
- caps: `semantic_limit <= 1000`, `lexical_fanout <= 2000`, `semantic_fanout <= 1000`

Required metadata fields:
- `semantic_limit_used`
- `lexical_fanout_used`
- `semantic_fanout_used`
- `semantic_budget_exhausted`

`semantic_budget_exhausted` MUST be `true` when the semantic branch reaches the effective semantic fanout cap and may be recall-limited.

#### Scenario: Excessive multiplier input is clamped and observable
- **WHEN** config multipliers produce `semantic_fanout > 1000`
- **THEN** runtime MUST clamp to `semantic_fanout_used=1000`
- **AND** metadata MUST expose the clamped value

#### Scenario: Budget exhaustion is non-fatal and visible
- **WHEN** semantic candidates hit the effective capped fanout limit
- **THEN** search MUST still return valid results
- **AND** metadata MUST set `semantic_budget_exhausted=true`

### Requirement: Deterministic fail-soft reason codes and recovery
Semantic failure fallback MUST use deterministic reason codes and MUST recover automatically when backend health returns.

Failure reason codes (runtime contract set):
- `semantic_backend_error` (required)
- `semantic_backend_timeout` (reserved)
- `semantic_backend_unavailable` (reserved)

Behavior:
- runtime MUST emit structured warning logs on semantic failure fallback
- runtime MUST keep serving lexical results
- runtime MUST resume semantic retrieval without process restart once backend is available

#### Scenario: Recovery after transient backend outage
- **WHEN** a query previously degraded with `semantic_backend_error`
- **AND** a later query runs after backend health is restored
- **THEN** semantic execution MUST proceed normally without restart/reindex side effects

### Requirement: SQLite semantic scale-tier contract
Semantic vector execution over SQLite MUST expose tiered operational guidance by vector cardinality per `(project_id, ref)`.

Tiers:
- Tier 1: `< 50k` vectors (supported baseline)
- Tier 2: `50k–200k` vectors (degraded-latency warning tier)
- Tier 3: `> 200k` vectors (unsupported-for-SLO warning tier)

Required behavior:
- Tier 2+ MUST emit warnings (non-fatal)
- Tier 3 warning MUST include migration guidance to ANN backend (`search.semantic.embedding.vector_backend = "lancedb"`)

#### Scenario: Tier-2 repository emits degradation warning
- **WHEN** semantic vector count for `(project_id, ref)` enters `50k–200k`
- **THEN** runtime/indexing metadata or logs MUST include a degraded-latency warning

#### Scenario: Tier-3 repository emits migration guidance
- **WHEN** semantic vector count exceeds `200k`
- **THEN** warning output MUST include ANN migration guidance (`lancedb` backend)

### Requirement: Benchmark-gated acceptance profile for Tier-1 corpora
Release acceptance for this capability MUST be evaluated using `crates/cruxe-query/examples/semantic_benchmark_eval.rs` report output.

Required report extensions:
- degraded-query rate
- semantic budget exhaustion rate

Reference acceptance profile (Tier-1 corpus, reference hardware/profile):
- p95 latency target: `<= 500ms`
- zero-result rate and MRR MUST be reported as release evidence

The 500ms target is a benchmark-profile contract and MUST NOT be implemented as a hard runtime assertion for all machines.

#### Scenario: Benchmark report includes semantic quality counters
- **WHEN** benchmark evaluation is executed for release verification
- **THEN** output MUST include degraded-query rate and semantic budget-exhaustion rate in addition to latency/MRR/zero-result metrics
