# context-pack-builder Evidence

Date: 2026-02-28
Worktree: `.worktrees/context-pack-builder`

## Verification Commands

### 1) Full workspace validation

```bash
cargo test --workspace
cargo clippy --workspace
```

Result:
- `cargo test --workspace`: PASS
- `cargo clippy --workspace`: PASS

### 2) Context-pack focused verification

```bash
cargo test -p cruxe-query context_pack::tests -- --nocapture
cargo test -p cruxe-mcp build_context_pack -- --nocapture
cargo test -p cruxe-mcp t363_context_pack_iterative_fixture_drives_followup_query_loop -- --nocapture
cargo test -p cruxe-mcp t366_build_context_pack_zero_results_emits_underfilled_guidance -- --nocapture
```

Result:
- `cruxe-query` context-pack unit tests: PASS (8/8)
- MCP integration tests for `build_context_pack`: PASS (6/6), including:
  - `t364_build_context_pack_accepts_partial_section_caps_patch`
  - `t365_build_context_pack_enforces_max_candidates_upper_bound`
  - `t366_build_context_pack_zero_results_emits_underfilled_guidance`
- Iterative fixture workflow test (`pack -> follow-up pack`): PASS

## Sample Pack Contract (excerpt)

Representative response shape from the implemented MCP contract:

```json
{
  "query": "validate_token",
  "ref": "main",
  "mode": "full",
  "budget_tokens": 280,
  "token_budget_used": 246,
  "sections": {
    "definitions": [
      {
        "snippet_id": "symbol:test-repo:main:src/handler.rs:10:24:...",
        "ref": "main",
        "path": "src/handler.rs",
        "line_start": 10,
        "line_end": 24,
        "content_hash": "...",
        "selection_reason": "primary:definition",
        "estimated_tokens": 38
      }
    ],
    "usages": [],
    "deps": [],
    "tests": [],
    "config": [],
    "docs": []
  },
  "dropped_candidates": 3,
  "coverage_summary": {
    "section_counts": {
      "definitions": 1,
      "usages": 0,
      "deps": 0,
      "tests": 0,
      "config": 0,
      "docs": 0
    }
  },
  "suggested_next_queries": [
    "validate_token call sites",
    "validate_token imports"
  ]
}
```

## Determinism / Budget Evidence

- Determinism guard: `t362_build_context_pack_is_deterministic_across_repeated_calls`
  - Asserts full JSON payload equality for repeated identical requests.
- Budget guard: `t360_build_context_pack_returns_sectioned_provenance_payload`
  - Asserts `token_budget_used <= budget_tokens`.
- Candidate bound guard: `t365_build_context_pack_enforces_max_candidates_upper_bound`
  - Asserts selected and raw candidate counts do not exceed `max_candidates`.
- Underfilled guidance guard: `t366_build_context_pack_zero_results_emits_underfilled_guidance`
  - Asserts zero-result runs include explicit expansion hints and follow-up queries.
- Iterative retrieval guard: `t363_context_pack_iterative_fixture_drives_followup_query_loop`
  - Uses fixture `testdata/fixtures/context-pack/iterative-workflow.json` and validates low-budget pack produces actionable follow-up query.

## Follow-up Repair Evidence

- Query-layer accepts partial section cap patches via `SectionCapsPatch` merge behavior.
- MCP handler now accepts partial `section_caps` objects and merges with mode defaults.
- Dedup key includes symbol + span to avoid collapsing distinct spans of the same symbol.
- Candidate pre-dedup truncation now enforces `max_candidates` as a hard upper bound.
- Section classification moved into `context_pack/sectioning.rs` to keep assembly logic focused.
- Metadata now exposes `budget_utilization_ratio`, token estimation method, and `aider_minimal` alias mapping.

## Compatibility Evidence

- Continue-style section compatibility validated in `t360...`:
  - Metadata alias: `key_usages -> usages`
  - Metadata alias: `dependencies -> deps`
