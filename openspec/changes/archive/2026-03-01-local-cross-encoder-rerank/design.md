## Context

Current rerank quality is constrained by lexical overlap. External rerank APIs improve semantic relevance but add network dependency and operational variability. We need a local semantic rerank layer that remains universal and robust.

## Goals / Non-Goals

**Goals**
1. Add local cross-encoder reranking without per-language logic.
2. Keep reranking fail-soft with deterministic fallback.
3. Control latency and memory impact with explicit budget knobs.
4. Validate improvement with benchmark gates.

**Non-Goals**
1. Training domain-specific reranker models.
2. GPU-specific optimization in Phase 1.
3. Per-language reranker policies.

## Decisions

### D1. fastembed as local rerank backend

#### Decision

Implement `LocalCrossEncoderReranker` using `fastembed::TextRerank` with lazy initialization.

- default model: `rozgo/bge-reranker-v2-m3`
- configurable model + max length

#### Rationale

fastembed exists at workspace level; this change explicitly adds it to `cruxe-query` for rerank integration.

### D2. Bounded rerank candidate budget

#### Decision

Introduce rerank input caps:

- `rerank_candidate_cap` (e.g., default 50)
- `rerank_top_n` from existing flow

Only top lexical/hybrid candidates are reranked by cross-encoder.

#### Rationale

Controls p95 latency and memory use while preserving quality gains.

### D3. Fail-soft fallback chain

#### Decision

If cross-encoder load/inference fails, fallback to `LocalRuleReranker` with structured reason codes:

- `cross_encoder_model_load_failed`
- `cross_encoder_inference_failed`
- `cross_encoder_timeout`

#### Rationale

Search reliability is more important than semantic rerank availability.

### D4. Benchmark and rollout gate

#### Decision

Add explicit acceptance gates before recommending `cross-encoder` as default:

- quality gate: NDCG@10 and/or MRR@10 uplift vs lexical rerank baseline,
- latency gate: p95 increase within configured bound,
- fallback rate gate: below threshold under normal conditions.

#### Rationale

Prevents regressions and avoids subjective tuning.

## Risks / Trade-offs

- **Risk: model size/startup cost (hundreds of MB to ~1GB+).**
  - Mitigation: lazy load, model configurability, fallback path.

- **Risk: semantic model not code-specialized.**
  - Mitigation: benchmark gate + keep lexical signals in upstream retrieval.

- **Trade-off: extra CPU cost per query.**
  - Accepted with candidate cap and latency guardrails.
