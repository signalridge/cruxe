## 1. Diversity algorithm (cruxe-query)

- [x] 1.1 Implement `apply_file_diversity` with sliding-window constraints.
- [x] 1.2 Enforce score-floor check (`min_score_ratio`) before any promotion.
- [x] 1.3 Preserve within-file relative order during rotations.

## 2. Pipeline integration (cruxe-query)

- [x] 2.1 Insert diversity pass between final ranking sort and truncate.
- [x] 2.2 Ensure ranking reason alignment remains correct after reordering.
- [x] 2.3 Guard execution with `diversity_enabled` flag.

## 3. MCP parameter plumbing (cruxe-mcp)

- [x] 3.1 Keep/add `diversity: bool` parameter in `search_code` schema.
- [x] 3.2 Wire parameter into query `SearchOptions`.
- [x] 3.3 Document behavior in protocol/tool docs.

## 4. Tests

- [x] 4.1 Unit tests for same-file clustering mitigation.
- [x] 4.2 Unit tests for no-change on already-diverse ranking.
- [x] 4.3 Unit tests for score-floor protection.
- [x] 4.4 Unit tests for `diversity: false` passthrough behavior.

## 5. Benchmark gate and verification

- [x] 5.1 Add diversity benchmark report fields: unique_files@k, max_file_share@k.
- [x] 5.2 Compare relevance metrics (NDCG@10/MRR@10) with diversity on/off.
- [x] 5.3 Define pass criteria (diversity gain with bounded relevance regression).
- [x] 5.4 Run `cargo test --workspace` and `cargo clippy --workspace`.
- [x] 5.5 Update OpenSpec evidence with benchmark results.

## Dependency order

```
1 (algorithm) → 2 (pipeline) + 3 (MCP param) → 4 (tests) → 5 (benchmark/verification)
```
