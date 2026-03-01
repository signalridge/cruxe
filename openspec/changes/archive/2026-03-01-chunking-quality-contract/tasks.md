## 1. Symbol-origin chunk quality (cruxe-indexer)

- [x] 1.1 Implement overlap-aware splitting for large symbol bodies.
- [x] 1.2 Implement token-budget guard and truncation metadata.
- [x] 1.3 Keep small symbols as single chunks.

## 2. File-fallback chunking (cruxe-indexer)

- [x] 2.1 Detect files with zero extracted symbols.
- [x] 2.2 Generate fallback chunks from raw file text using shared chunk settings.
- [x] 2.3 Mark fallback chunk origin as `file_fallback`.
- [x] 2.4 Add tests for unsupported-language file producing fallback chunks.

## 3. Schema and metadata (cruxe-state)

- [x] 3.1 Add/confirm metadata fields: `origin`, `parent_symbol_stable_id` (nullable), `chunk_index`, `line_start`, `line_end`, `truncated`.
- [x] 3.2 Ensure vector writer populates metadata for both origins.

## 4. Retrieval and dedup (cruxe-query)

- [x] 4.1 Implement origin-aware dedup rules (symbol parent vs fallback span).
- [x] 4.2 Prefer symbol-origin chunk for display when both origins overlap.
- [x] 4.3 Add explain metadata showing chunk origin.

## 5. Configuration (cruxe-core)

- [x] 5.1 Keep shared chunk settings under `search.semantic.chunking`.
- [x] 5.2 Ensure settings apply uniformly to both origins.

## 6. Verification

- [x] 6.1 Run `cargo test --workspace`.
- [x] 6.2 Run `cargo clippy --workspace`.
- [x] 6.3 Compare semantic recall on a mixed-language fixture (with and without fallback enabled).
- [x] 6.4 Update OpenSpec evidence with vector count and quality impact.

## Dependency order

```
1 (symbol-origin quality) + 2 (file-fallback) → 3 (schema) → 4 (retrieval/dedup) → 5 (config) → 6 (verification)
```
