## ADDED Requirements

### Requirement: File-spread diversity reranking
The search pipeline MUST apply a file-spread diversity pass after scoring and before result truncation. The pass ensures that top results are not dominated by a single file.

Algorithm:
- Iterate ranked results from position 0 to `limit`.
- For each result at position N, check the preceding `effective_window`, where `effective_window = min(window_size, total_result_count)`.
- If more than `max_per_file` results in the window come from the same file path, swap the current result with the next result from a different file.
- Default parameters: `window_size = 5`, `max_per_file = 2`.
- Complexity: O(n × W) where n is result count and W is `window_size`. This is intentionally simpler than MMR (O(k²·n)) — the goal is preventing pathological clustering, not optimizing information-theoretic novelty.

Design rationale — conservative by intent:
- Zoekt's only diversity mechanism (`boostNovelExtension`) operates on **file extension**, not file path, and promotes exactly 1 result. cruxe's file-path–based approach is strictly more granular.
- Research consensus (Sourcegraph, GitHub Blackbird, Livegrep): aggressive diversity harms code search — same-file results are often exactly what the user needs. The `max_per_file=2` default allows moderate clustering.
- Unlike Elasticsearch `collapse` (which hard-deduplicates to 1 per field value), this approach preserves high-scoring same-file results beyond the window.

The diversity pass MUST preserve relative score ordering within the same file — it only reorders across files, never within.

#### Scenario: Same-file results redistributed
- **WHEN** top-5 results are all from `src/handler.rs` with scores [10.0, 9.5, 9.0, 8.5, 8.0]
- **AND** the 6th result is from `src/service.rs` with score 7.5
- **THEN** after diversity pass with `max_per_file=2`, the 3rd position MUST be replaced by the `src/service.rs` result
- **AND** the displaced `src/handler.rs` results shift down

#### Scenario: Promoted result preserves reasonable score proximity
- **WHEN** a swap candidate from a different file has score S_swap and the result it would replace has score S_displaced
- **THEN** the swap MUST only occur if `S_swap >= S_displaced * 0.5` (minimum score ratio)
- **AND** if no candidate meets the threshold, the original ordering MUST be preserved at that position
- **NOTE** This prevents promoting a score-1.0 result to replace a score-10.0 result just for diversity (analogous to Zoekt's 0.9× threshold on `boostNovelExtension`, relaxed for file-path granularity)

#### Scenario: Already diverse results unchanged
- **WHEN** top-5 results come from 4 different files
- **THEN** the diversity pass MUST NOT change the ordering

#### Scenario: Diversity disabled returns pure score ordering
- **WHEN** `diversity: false` is specified in the search request
- **THEN** the diversity pass MUST be skipped entirely
- **AND** results are ordered strictly by score

#### Scenario: Small result set still applies bounded diversity
- **WHEN** the total result count is less than `window_size`
- **THEN** the diversity pass MUST use `effective_window = total_result_count`
- **AND** MAY reorder across files if `max_per_file` and score-ratio constraints are violated

### Requirement: search_code supports diversity parameter
The `search_code` MCP tool MUST accept an optional `diversity` boolean parameter.

- `diversity: true` (default): file-spread diversity pass is applied.
- `diversity: false`: diversity pass is skipped; pure score ordering.

#### Scenario: Default search applies diversity
- **WHEN** `search_code` is called without a `diversity` parameter
- **THEN** the file-spread diversity pass MUST be applied (default true)

#### Scenario: Explicit false disables diversity
- **WHEN** `search_code` is called with `diversity: false`
- **THEN** results MUST be in pure score order without file-spread reranking
