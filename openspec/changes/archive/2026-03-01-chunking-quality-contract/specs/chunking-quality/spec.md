## ADDED Requirements

### Requirement: Large symbol bodies split into overlapping sub-chunks
Symbol bodies exceeding `MAX_CHUNK_LINES` (default 50) MUST be split into overlapping sub-chunks for vector embedding. Small symbols (≤ threshold) remain as single chunks.

Parameters:
- `MAX_CHUNK_LINES`: threshold for splitting (default 50, configurable).
- `CHUNK_SIZE`: lines per sub-chunk (default 40, configurable).
- `CHUNK_OVERLAP`: overlap lines between adjacent sub-chunks (default 10, configurable).

Each sub-chunk retains metadata linking it to the parent symbol.

#### Scenario: Large function split into sub-chunks
- **WHEN** a function body spans 120 lines (exceeds `MAX_CHUNK_LINES=50`)
- **THEN** the extractor MUST produce sub-chunks: lines 1-40, lines 31-70, lines 61-100, lines 91-120
- **AND** each sub-chunk MUST reference the parent `symbol_stable_id`

#### Scenario: Small function remains as single chunk
- **WHEN** a function body spans 30 lines (≤ `MAX_CHUNK_LINES=50`)
- **THEN** the extractor MUST produce exactly one chunk containing the full body (existing behavior)

#### Scenario: Overlap preserves cross-boundary context
- **WHEN** a function has important logic spanning lines 38-42
- **THEN** this logic MUST appear in at least two sub-chunks (chunk 1: lines 1-40, chunk 2: lines 31-70)
- **AND** a vector query matching this logic MUST have at least two candidate chunks to match against
- **NOTE** Continue uses a mixed strategy (`codeChunker` with fallback to `basicChunker`), while cruxe's intra-symbol splitting is explicitly line-window based; overlap is required because split boundaries are inside a single symbol body rather than natural top-level AST boundaries

#### Scenario: Chunk parameters are configurable
- **WHEN** `search.semantic.chunking.max_chunk_lines = 80` is set in configuration
- **THEN** only symbols exceeding 80 lines MUST be split

### Requirement: Token budget awareness for embedding
The chunking pipeline MUST detect when a chunk's approximate token count exceeds the embedding model's maximum context window and truncate accordingly.

Token estimation: `approximate_tokens = text.len() / 4` (byte-to-token heuristic). This ratio is optimistic for code — identifiers, operators, and mixed-case naming cause tokenizers to produce more tokens per byte than natural language. Empirical accuracy is approximately 75-85% for code text (i.e., actual tokens may be 15-25% higher than estimate).

Behavior:
- Chunks within token budget: embedded as-is.
- Chunks exceeding token budget: truncated to `max_tokens * 4` bytes, with `truncated = true` metadata.
- Token budget: 512 tokens (NomicEmbedTextV15Q effective limit — the model architecture supports 8192 tokens, but FastEmbed's integration may silently truncate or degrade quality beyond 512).

#### Scenario: Normal chunk embedded without truncation
- **WHEN** a sub-chunk has 800 bytes (approximately 200 tokens, within 512-token limit)
- **THEN** it MUST be embedded as-is with `truncated = false`

#### Scenario: Oversized chunk truncated with metadata
- **WHEN** a sub-chunk has 3000 bytes (approximately 750 tokens by `bytes/4` heuristic, exceeds 512-token limit)
- **THEN** it MUST be truncated to approximately 2048 bytes (512 × 4)
- **AND** `truncated = true` MUST be recorded in `semantic_vectors`
- **AND** a warning MUST be logged
- **NOTE** The `bytes/4` heuristic underestimates code token count by ~15-25%. A 2048-byte chunk may contain ~600 actual tokens, still within typical model tolerance. Future improvement: use the tokenizer directly for precise counting.

### Requirement: Sub-chunk metadata preserves parent symbol linkage
Each sub-chunk stored in `semantic_vectors` MUST include metadata linking it to the originating symbol and its position within that symbol.

Required metadata fields:
- `parent_symbol_stable_id`: the symbol that was split.
- `chunk_index`: 0-based index of this sub-chunk within the parent.
- `line_start`, `line_end`: actual line range of this sub-chunk (not the full symbol).

#### Scenario: Search result points to specific sub-chunk range
- **WHEN** a semantic search matches a sub-chunk covering lines 61-100 of a 120-line function
- **THEN** the search result MUST report `line_start=61, line_end=100` (sub-chunk range, not full symbol range)

#### Scenario: Multiple sub-chunks from same symbol deduplicated in results
- **WHEN** two sub-chunks from the same parent symbol both match a semantic query
- **THEN** the dedup logic MUST merge them into a single result using the parent `symbol_stable_id`
- **AND** the highest-scoring sub-chunk's line range MUST be used for the merged result
