## 1. Centrality computation (cruxe-indexer)

- [ ] 1.1 Implement relation centrality query over `symbol_edges` + `symbol_relations` (resolved, inter-file edges only).
- [ ] 1.2 Max-normalize centrality into `[0.0, 1.0]`.
- [ ] 1.3 Add tests for empty graph, self-edge exclusion, and normalization correctness.

## 2. Index schema and writer (cruxe-state)

- [ ] 2.1 Add `file_centrality` FAST f64 field to Tantivy symbol schema.
- [ ] 2.2 Pass centrality map into symbol document writer.
- [ ] 2.3 Persist centrality value per symbol record by file path.

## 3. Ranking integration (cruxe-query)

- [ ] 3.1 Load `file_centrality` from Tantivy hit.
- [ ] 3.2 Add bounded `centrality_boost` term (`CENTRALITY_WEIGHT = 1.0` default).
- [ ] 3.3 Ensure centrality term cannot override exact match behavior in tests.

## 4. Explain integration (cruxe-query)

- [ ] 4.1 Add centrality fields to scoring breakdown and details.
- [ ] 4.2 Add human-readable reason text with raw + weighted values.

## 5. Verification

- [ ] 5.1 Run `cargo test --workspace`.
- [ ] 5.2 Run `cargo clippy --workspace`.
- [ ] 5.3 Validate ranking deltas on fixture where lexical scores tie but structure differs.
- [ ] 5.4 Update OpenSpec evidence with before/after ranking examples.

## Dependency order

```
1 (centrality computation) → 2 (schema/writer) → 3 (ranking) → 4 (explain) → 5 (verification)
```
