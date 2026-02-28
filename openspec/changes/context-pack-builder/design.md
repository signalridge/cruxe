## Context

Current `search_code`/`locate_symbol` output is hit-centric. Agents often need an additional orchestration step to collect minimal sufficient context. Doing this in clients duplicates logic and leads to inconsistent token usage.

## Goals / Non-Goals

**Goals:**
1. Provide first-party `build_context_pack` with deterministic output sections.
2. Enforce explicit token budget while maximizing relevance coverage.
3. Emit provenance envelope for each snippet.
4. Return actionable follow-up suggestions for iterative retrieval.

**Non-Goals:**
1. Auto-editing code as part of pack generation.
2. Full document summarization service.
3. Provider-specific prompt formatting in this phase.

## Decisions

### D1. Pack assembly pipeline
Pipeline:
1) retrieve candidates,
2) cluster/dedup by symbol + file span,
3) section assignment,
4) budgeted selection,
5) serialize with provenance.

Section assignment rules (deterministic):
- `definitions`: primary symbol definition spans.
- `key_usages`: inbound/outbound usage or call/reference spans.
- `dependencies`: import/include/dependency declarations.
- `tests`: paths/snippets matched by test path heuristics.
- `config`: recognized config/build files (e.g., `*.toml`, `*.yaml`, build manifests).
- `docs`: markdown/docs comments/readme-class sources.

Each snippet MUST map to exactly one section using first-match rule priority:
`definitions > key_usages > dependencies > tests > config > docs`.

**Why:** deterministic and composable.

### D2. Budget controller with deterministic truncation
Use a shared token estimation heuristic and deterministic priority order per section.

**Why:** predictable behavior across clients.

### D3. Provenance envelope contract
Every pack item includes:
- stable `snippet_id`
- `ref`
- `path` + `line_start`/`line_end`
- `content_hash`
- `selection_reason`

**Why:** enables auditability and refresh checks.

### D4. Guidance metadata
Add `suggested_next_queries` and `missing_context_hints` when budget/coverage is insufficient.

**Why:** helps agent continue retrieval loop efficiently.

## Risks / Trade-offs

- **[Risk] Pack assembly latency overhead** → Mitigation: bounded candidate fanout and cacheable intermediate results.
- **[Risk] Token estimation mismatch with downstream model tokenizer** → Mitigation: conservative headroom and configurable estimator.
- **[Risk] Section overfitting (one section dominates)** → Mitigation: per-section caps and balancing rules.

## Migration Plan

1. Implement behind feature/config flag.
2. Shadow-run pack builder using existing search outputs for validation.
3. Enable MCP tool once deterministic output is verified.
4. Document client integration examples.

Rollback: keep tool disabled while preserving internals for diagnostics.

## Resolved Defaults

1. Section weights are fixed in phase 1 (no intent-specific weighting tables).
2. Phase 1 uses one generic token estimator with conservative headroom; model-specific estimators are deferred.

## External References (2026-02-28 Investigation)

Investigated related open-source projects and extracted directly applicable design constraints:

- **continuedev/continue** (TypeScript, stars=31569)
  - Upstream focus: ⏩ Source-controlled AI checks, enforceable in CI. Powered by the open-source Continue CLI
  - Local clone: `<ghq>/github.com/continuedev/continue`
  - Applied insight: Agent-facing context packaging and IDE pipeline integration patterns.
  - Source: https://github.com/continuedev/continue
- **aider-ai/aider** (Python, stars=41041)
  - Upstream focus: aider is AI pair programming in your terminal
  - Local clone: `<ghq>/github.com/Aider-AI/aider`
  - Applied insight: Terminal-agent minimal context and edit-focused packing heuristics.
  - Source: https://github.com/Aider-AI/aider
- **run-llama/llama_index** (Python, stars=47259)
  - Upstream focus: LlamaIndex is the leading document agent and OCR platform
  - Local clone: `<ghq>/github.com/run-llama/llama_index`
  - Applied insight: Retriever composition and context window budgeting tactics.
  - Source: https://github.com/run-llama/llama_index
