## 1. Symbol-origin chunk quality (cruxe-indexer)

- [ ] 1.1 Implement overlap-aware splitting for large symbol bodies.
- [ ] 1.2 Implement token-budget guard and truncation metadata.
- [ ] 1.3 Keep small symbols as single chunks.

## 2. File-fallback chunking (cruxe-indexer)

- [ ] 2.1 Detect files with zero extracted symbols.
- [ ] 2.2 Generate fallback chunks from raw file text using shared chunk settings.
- [ ] 2.3 Mark fallback chunk origin as `file_fallback`.
- [ ] 2.4 Add tests for unsupported-language file producing fallback chunks.

## 3. Schema and metadata (cruxe-state)

- [ ] 3.1 Add/confirm metadata fields: `origin`, `parent_symbol_stable_id` (nullable), `chunk_index`, `line_start`, `line_end`, `truncated`.
- [ ] 3.2 Ensure vector writer populates metadata for both origins.

## 4. Retrieval and dedup (cruxe-query)

- [ ] 4.1 Implement origin-aware dedup rules (symbol parent vs fallback span).
- [ ] 4.2 Prefer symbol-origin chunk for display when both origins overlap.
- [ ] 4.3 Add explain metadata showing chunk origin.

## 5. Configuration (cruxe-core)

- [ ] 5.1 Keep shared chunk settings under `search.semantic.chunking`.
- [ ] 5.2 Ensure settings apply uniformly to both origins.

## 6. Verification

- [ ] 6.1 Run `cargo test --workspace`.
- [ ] 6.2 Run `cargo clippy --workspace`.
- [ ] 6.3 Compare semantic recall on a mixed-language fixture (with and without fallback enabled).
- [ ] 6.4 Update OpenSpec evidence with vector count and quality impact.

## Dependency order

```
1 (symbol-origin quality) + 2 (file-fallback) → 3 (schema) → 4 (retrieval/dedup) → 5 (config) → 6 (verification)
```
