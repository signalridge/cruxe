## Context

Current retrieval flow is largely uniform. With cross-encoder rerank and richer graph signals, always-on heavy paths can raise p95 latency and increase resource variance. We need deterministic adaptive planning rather than ad-hoc branching.

## Goals / Non-Goals

**Goals:**
1. Select retrieval plan deterministically from query intent and confidence.
2. Bound each plan with explicit budget and timeout behavior.
3. Preserve fail-soft semantics with transparent downgrade metadata.
4. Keep plan selection inspectable and configurable.

**Non-Goals:**
1. Reinforcement learning or dynamic online policy training.
2. User-specific personalization.
3. Cross-process orchestrator complexity.

## Decisions

### D1. Three canonical plans
- `lexical_fast`: lexical retrieval + lightweight ranking only.
- `hybrid_standard`: lexical + semantic candidate merge + local rerank.
- `semantic_deep`: expanded fanout + cross-encoder rerank + diversity.

**Why:** enough expressive power without policy sprawl.

### D2. Rule-first deterministic selector
Selector inputs: intent class, lexical confidence, semantic availability, explicit request overrides.

Deterministic rule order (first match wins):
1. Explicit override (`plan=...`) if allowed by config.
2. If semantic runtime unavailable -> `lexical_fast` for symbol/path intents, otherwise `hybrid_standard`.
3. If intent in `{symbol, path, error}` and lexical confidence >= `0.75` -> `lexical_fast`.
4. If intent in `{natural_language, exploratory}` and lexical confidence < `0.55` and semantic runtime available -> `semantic_deep`.
5. Default -> `hybrid_standard`.

Downgrade reasons are fixed enums (for example `semantic_unavailable`, `budget_exhausted`, `timeout_guard`, `config_forced`).

**Why:** predictable and debuggable; avoids opaque policy drift.

### D3. Budget and downgrade controller
Each plan has max fanout and latency target. Runtime can downgrade only in one direction (deep→standard→fast) with explicit reason.

**Why:** fail-soft behavior while preserving bounded resources.

### D4. Metadata and explain integration
Expose:
- `query_plan_selected`
- `query_plan_downgraded`
- `query_plan_downgrade_reason`
- `query_plan_budget_used`

**Why:** transparent operations and easier incident analysis.

## Risks / Trade-offs

- **[Risk] Misclassification can under-search difficult queries** → Mitigation: override flag + conservative thresholds + eval coverage.
- **[Risk] More config complexity** → Mitigation: sensible defaults and config lint.
- **[Risk] Plan switching creates behavior drift** → Mitigation: deterministic selector and regression gate snapshots.

## Migration Plan

1. Introduce selector in shadow mode (report-only metadata).
2. Validate on retrieval-eval-gate suite.
3. Enable active planning for default queries.
4. Enable optional per-request override once stable.

Rollback: force all requests to `hybrid_standard` via config toggle.

## Resolved Defaults

1. `semantic_deep` is available in phase 1 for all repo sizes, gated by runtime budgets/availability.
2. Thresholds are universal in phase 1 (no language-family threshold tables).

## External References (2026-02-28 Investigation)

Investigated related open-source projects and extracted directly applicable design constraints:

- **deepset-ai/haystack** (MDX, stars=24349)
  - Upstream focus: Open-source AI orchestration framework for building context-engineered, production-ready LLM applications. Design modular pipelines and agent workflows with explicit control over retrieval, routing, memory, and generation. Built for scalable agents, RAG, multimodal applications, semantic search, and conversational systems.
  - Local clone: `<ghq>/github.com/deepset-ai/haystack`
  - Applied insight: Pipeline routing patterns and budget-aware orchestration design.
  - Source: https://github.com/deepset-ai/haystack
- **run-llama/llama_index** (Python, stars=47259)
  - Upstream focus: LlamaIndex is the leading document agent and OCR platform
  - Local clone: `<ghq>/github.com/run-llama/llama_index`
  - Applied insight: Query-router and retrieval-mode selection patterns for ambiguous prompts.
  - Source: https://github.com/run-llama/llama_index
- **continuedev/continue** (TypeScript, stars=31569)
  - Upstream focus: ⏩ Source-controlled AI checks, enforceable in CI. Powered by the open-source Continue CLI
  - Local clone: `<ghq>/github.com/continuedev/continue`
  - Applied insight: Agent workflow constraints for practical latency/quality tradeoffs.
  - Source: https://github.com/continuedev/continue
