---
id: testing-strategy
title: "Testing Strategy"
status: active
scope: cross-cutting (all specs)
refs:
  - "design.md"
  - "benchmark-targets.md"
  - "repo-maintenance.md"
migrated_from: plan/verify/testing-strategy.md
---

# Testing Strategy

> Authoritative cross-spec testing strategy.
> Quantitative thresholds are defined in [benchmark-targets.md](benchmark-targets.md).

## Test Layers

### 1. Unit Tests

| Concern | Primary Spec | Examples |
|---------|--------------|----------|
| Query intent classification | 001-core-mvp | `symbol` / `path` / `error` / `natural_language` |
| Ranking rules | 001-core-mvp | deterministic lexical and rule boosts |
| Structural path boost | 002-agent-protocol | penalties/bonuses by path pattern and multiplicative behavior |
| Compact serialization | 002-agent-protocol | `detail_level` + `compact` interaction and field guarantees |
| Explainability levels | 002-agent-protocol | `ranking_explain_level` (`off`/`basic`/`full`) payload gating |
| Parser extraction | 001-core-mvp | language-specific symbol extraction |
| Tokenizers | 001-core-mvp | `code_camel`, `code_snake`, `code_dotted`, `code_path` |
| Symbol graph traversal | 003-structure-nav | hierarchy and related-symbol traversal |
| Token budget estimation | 003-structure-nav | context sizing and truncation behavior |
| Result dedup and hard limits | 002-agent-protocol | duplicate suppression, truncation metadata, graceful degrade |
| Intent confidence calibration | 008-semantic-hybrid | `query_intent_confidence` thresholds and escalation triggers |
| VCS merge semantics | 005-vcs-core | merge keys, tombstones, overlay precedence |
| Tooling determinism | 006-vcs-ga-tooling | stable `explain_ranking` outputs |

### 2. Integration Tests

| Concern | Primary Spec | Coverage |
|---------|--------------|----------|
| Index pipeline end-to-end | 001-core-mvp | scan -> parse -> write -> query |
| MCP contract conformance | 001-core-mvp and later tool specs | schema/shape validation |
| `detail_level` serialization | 002-agent-protocol | response shape by verbosity |
| Metadata enum conformance | meta + all contracts | `indexing_status` and `result_completeness` canonical values |
| Diff/ref tooling behavior | 006-vcs-ga-tooling | `diff_context`, `find_references`, `switch_ref` |
| Call graph extraction/query | 007-call-graph | call edges and traversal correctness |
| Hybrid fallback behavior | 008-semantic-hybrid | provider failure -> local fallback |
| Watch daemon lifecycle | 004-workspace-transport | `watch --background/--status/--stop` and readiness behavior |
| Interrupted job recovery status | 004-workspace-transport | restart leaves clear `interrupted_recovery_report` in `index_status`/`health_check` |
| Profile advisor output | 008-semantic-hybrid | recommendation consistency by repo-size/language buckets |

### 3. End-to-End Tests

| Concern | Primary Spec | Acceptance Focus |
|---------|--------------|------------------|
| Location accuracy | 001-core-mvp | precise `file:line` answers |
| Branch overlay correctness | 005-vcs-core | no cross-ref leakage |
| GA tooling workflows | 006-vcs-ga-tooling | branch diff/ref workflows |
| Multi-workspace routing | 004-workspace-transport | workspace-scoped result isolation |
| Workspace warmset behavior | 004-workspace-transport | recent workspaces prewarmed, cold workspace fallback correctness |
| Context budget truncation | 003-structure-nav | `max_tokens` never exceeded |
| Agent compact flow | 002-agent-protocol | same query in compact/non-compact preserves ordering and follow-up handles |

### 4. Relevance & Performance Benchmarks

| Bucket | Example Queries | Primary Metric |
|--------|-----------------|----------------|
| symbol | `validate_token`, `AuthHandler` | top-1 precision |
| path | `src/auth/handler.rs` | top-1 recall |
| error | `connection refused` | top-3 precision |
| natural_language | `how does auth work` | top-5 relevance + MRR uplift |

Additional benchmark assertions:

- structural boost reduces test/mock/generated over-ranking on mixed-intent suites,
- dedup improves top-k diversity without harming top-1 precision,
- semantic evaluation is reported per repo-size bucket (`<10k`, `10k-50k`, `>50k` files),
- benchmark kit re-runs on the same fixture/query pack should keep ranking metrics stable within configured drift tolerance.

For phase `008-semantic-hybrid`, the natural-language benchmark set should contain
at least 100 labeled queries, stratified across Rust/TypeScript/Python/Go
(minimum 20 queries per language).

## Test-to-Spec Traceability (Stable Anchors)

| Test Concern | Spec | Task Anchor |
|-------------|------|-------------|
| Core indexing/search correctness | 001-core-mvp | Phase 4-8 |
| Agent response shaping and health | 002-agent-protocol | Phase 2-7 |
| Structure/context tools | 003-structure-nav | Phase 4-7 |
| Workspace and transport | 004-workspace-transport | Phase 2-5 |
| VCS correctness core | 005-vcs-core | Phase 3-6 |
| VCS GA tooling and portability | 006-vcs-ga-tooling | Phase 1-6 |
| Call graph capabilities | 007-call-graph | Phase 2-6 |
| Semantic/hybrid + rerank | 008-semantic-hybrid | Phase 4-8 |
| Distribution validation | 009-distribution | Phase 1-6 |

## Test Data Strategy

| Fixture | Path | Purpose | Owner Spec |
|---------|------|---------|------------|
| `rust-sample/` | `testdata/fixtures/rust-sample/` | Rust symbol extraction | 001-core-mvp |
| `ts-sample/` | `testdata/fixtures/ts-sample/` | TypeScript extraction | 001-core-mvp |
| `python-sample/` | `testdata/fixtures/python-sample/` | Python extraction | 001-core-mvp |
| `go-sample/` | `testdata/fixtures/go-sample/` | Go extraction | 001-core-mvp |
| `vcs-sample/` | `testdata/fixtures/vcs-sample/` | Overlay correctness + VCS tooling | 005/006 |
| `call-graph-sample/` | `testdata/fixtures/call-graph-sample/` | Call graph extraction/query | 007-call-graph |

## CI Integration

- Unit + integration tests: every PR
- E2E suites: PR + merge-to-main
- Relevance/performance benchmarks: merge-to-main and release gates

## Regression Policy

Per [benchmark-targets.md](benchmark-targets.md):

- Latency regression > 20% on benchmark suite blocks merge
- Precision drop > 5% on relevance suite blocks merge
- Resource usage tracked continuously and reviewed in release readiness
