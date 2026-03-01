## Context

Cruxe currently builds semantic chunks from symbols. This is high precision where symbols exist, but it under-serves universality:

- languages with incomplete grammar coverage,
- non-code config files that still matter for retrieval,
- repositories where symbol extraction misses large regions.

The chunking system should produce high-quality chunks for both:

1. symbol-backed regions (preferred path),
2. file-backed fallback regions (universal safety net).

## Goals / Non-Goals

**Goals**
1. Keep overlap-aware chunking + token guard for symbol bodies.
2. Add deterministic fallback chunking when symbol coverage is absent.
3. Unify metadata so retrieval and dedup logic do not special-case language support.

**Non-Goals**
1. Language-specific chunking rules.
2. Full parser-aware block segmentation for every language.

## Decisions

### D1. Two-origin chunk generation

#### Decision

Chunk generation pipeline:

- **Origin A (`symbol`)**: existing symbol-body path, now with overlap-aware splitting.
- **Origin B (`file_fallback`)**: if a file yields zero symbols, chunk full file text using same line window contract.

#### Rationale

This ensures semantic retrieval never collapses to zero for unsupported/weakly-supported languages.

### D2. Shared chunking contract

#### Decision

Both origins use:

- `chunk_size_lines` (default 40),
- `chunk_overlap_lines` (default 10),
- `max_chunk_lines` threshold,
- token budget guard (`bytes/4` heuristic).

#### Rationale

One chunking contract keeps behavior predictable and tuneable.

### D3. Unified metadata

#### Decision

Each vector row records:

- `origin` (`symbol` or `file_fallback`),
- `parent_symbol_stable_id` (nullable for fallback),
- `chunk_index`,
- `line_start`, `line_end`,
- `truncated`.

#### Rationale

Supports origin-aware dedup and explainability while preserving one retrieval path.

### D4. Dedup policy

#### Decision

- Symbol-origin chunks dedup by `parent_symbol_stable_id`.
- Fallback-origin chunks dedup by `(path, line_start, line_end)`.
- If both origins match strongly in same region, prefer symbol-origin in final display.

#### Rationale

Keeps precision-first output while preserving fallback recall.

## Risks / Trade-offs

- **Risk: vector cardinality increase.**
  - Mitigation: enforce existing semantic budgets and caps.

- **Risk: fallback chunks may be noisier than symbol chunks.**
  - Mitigation: origin-aware scoring penalty for fallback chunks when symbol chunks are available.

- **Trade-off: more storage for universality.**
  - Accepted for significantly improved language-agnostic semantic coverage.
