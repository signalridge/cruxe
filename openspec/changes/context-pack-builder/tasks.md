## 1. Tool contract and request/response model

- [ ] 1.1 Define `build_context_pack` MCP tool schema and request parameters.
- [ ] 1.2 Define response model with sectioned items and provenance envelope.
- [ ] 1.3 Add compatibility tests for protocol serialization.

## 2. Pack assembly core

- [ ] 2.1 Implement candidate retrieval orchestration for pack building.
- [ ] 2.2 Implement dedup/clustering by symbol id and file span.
- [ ] 2.3 Implement section assignment (`definitions`, `usages`, `deps`, `tests`, `config`, `docs`).
- [ ] 2.4 Implement deterministic priority ordering within and across sections.

## 3. Budget controller and diagnostics

- [ ] 3.1 Implement token estimation + budget cutoff logic.
- [ ] 3.2 Add per-section caps and overflow handling.
- [ ] 3.3 Emit diagnostics (`token_budget_used`, `dropped_candidates`, `coverage_summary`).
- [ ] 3.4 Add `suggested_next_queries` for insufficient coverage scenarios.

## 4. MCP integration and examples

- [ ] 4.1 Wire tool into MCP server registry.
- [ ] 4.2 Add integration tests for ref-scoped pack generation.
- [ ] 4.3 Add docs and ready-to-use client examples.

## 5. Verification

- [ ] 5.1 Run `cargo test --workspace`.
- [ ] 5.2 Run `cargo clippy --workspace`.
- [ ] 5.3 Validate deterministic pack output on repeated runs.
- [ ] 5.4 Attach OpenSpec evidence with sample packs and budget behavior.

## 6. Agent workflow compatibility

- [ ] 6.1 Validate pack section schema against Continue context-provider expectations.
- [ ] 6.2 Add Aider-style minimal diff-focused pack mode for edit queries.
- [ ] 6.3 Add integration fixtures for iterative pack→follow-up-query workflows.
