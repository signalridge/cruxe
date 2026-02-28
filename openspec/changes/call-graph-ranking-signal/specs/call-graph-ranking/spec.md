## ADDED Requirements

### Requirement: File-level centrality derived from symbol edges
The indexing pipeline MUST compute a per-file centrality score from the `symbol_edges` table after all edges are written. The score measures how many distinct files reference symbols defined in a given file.

Centrality formula:
- `inbound_file_count`: number of distinct files that have edges pointing TO symbols defined in this file.
- `file_centrality = inbound_file_count / max_inbound_file_count` (max-normalized to 0.0-1.0 range).
- Files with zero inbound references receive `file_centrality = 0.0`.

#### Scenario: High-centrality file receives score near 1.0
- **WHEN** `src/db/pool.rs` defines symbols referenced from 15 other files (highest in the repo)
- **THEN** `file_centrality` for `src/db/pool.rs` MUST be 1.0 (15/15 = max)

#### Scenario: Low-centrality file receives proportional score
- **WHEN** `src/utils/helper.rs` defines symbols referenced from 3 other files, and max is 15
- **THEN** `file_centrality` for `src/utils/helper.rs` MUST be 0.2 (3/15)

#### Scenario: Unreferenced file receives zero centrality
- **WHEN** `src/tests/fixture.rs` defines symbols never referenced from other files
- **THEN** `file_centrality` for `src/tests/fixture.rs` MUST be 0.0

#### Scenario: Self-references excluded from centrality count
- **WHEN** `src/server.rs` has internal calls (within the same file) and 5 external file references
- **THEN** only the 5 external file references count toward `inbound_file_count`

### Requirement: Centrality boost in ranking formula
The ranking system MUST include `centrality_boost` as an additive signal in the reranking formula.

Formula: `centrality_boost = file_centrality * CENTRALITY_WEIGHT`

Where `CENTRALITY_WEIGHT = 1.0` (conservative tie-breaker; below major lexical boosts and exact-match contribution).

Updated ranking formula:
```
total_boost = exact_match_boost + qualified_name_boost + kind_weight
            + query_intent_boost + definition_boost + path_affinity
            + test_file_penalty + centrality_boost
```

Maximum additional boost: 1.0 (for the highest-centrality file in the repo).

#### Scenario: High-centrality symbol ranked above low-centrality equivalent
- **WHEN** two functions named `connect` have identical kind_weight (1.5) and other signals
- **AND** one is in `src/db/pool.rs` (centrality=1.0) and another in `src/tests/mock_db.rs` (centrality=0.0)
- **THEN** the `src/db/pool.rs` function MUST receive +1.0 centrality_boost and rank higher

#### Scenario: Centrality does not override exact match
- **WHEN** a variable `Pool` has an exact name match (5.0) in a low-centrality file (centrality=0.0)
- **AND** a class `PoolFactory` has no exact match in a high-centrality file (centrality=1.0)
- **THEN** the variable MUST still rank higher (exact_match dominates)

#### Scenario: Centrality boost appears in ranking explanation
- **WHEN** ranking explanation is requested via `explain_ranking`
- **THEN** the breakdown MUST include `centrality_boost` with the computed value and a reason string like `"file centrality boost (centrality=0.800, contribution=0.800)"`

### Requirement: Centrality materialized in Tantivy symbols index
The `file_centrality` value MUST be stored as a FAST f64 field in the Tantivy symbols index, populated at write time from the computed centrality table.

#### Scenario: Centrality field available for all indexed symbols
- **WHEN** a symbol is indexed in a file with computed centrality 0.6
- **THEN** the Tantivy document MUST contain `file_centrality: 0.6`

#### Scenario: Centrality recomputed on full reindex
- **WHEN** a full reindex is performed
- **THEN** all file centrality values MUST be recomputed from the current `symbol_edges` data

#### Scenario: Centrality is computed via in-degree counting (Phase 1)
- **WHEN** the centrality computation runs
- **THEN** it MUST use distinct-file in-degree counting via `symbol_edges.to_symbol_id -> symbol_relations.path` join (target file derived from callee symbol path, not a direct `target_file` column)
- **AND** the algorithm MUST be O(|E|) in the number of symbol edges (no iterative graph algorithms in Phase 1)
- **AND** future upgrade to PageRank (power iteration in Rust) MUST NOT require schema changes
