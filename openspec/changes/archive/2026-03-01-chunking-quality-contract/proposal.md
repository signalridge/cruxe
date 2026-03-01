## Why

Current semantic chunking quality work is strong, but still assumes semantic vectors originate from symbol bodies. This creates a universality gap:

- if a language has weak/partial symbol extraction, semantic coverage drops,
- unsupported file types receive little or no semantic recall,
- retrieval quality depends too much on language-specific extraction completeness.

To support broad language coverage without per-language maintenance, chunking must include a **generic fallback path**.

## What Changes

1. **Intra-symbol chunk quality improvements**
   - Keep overlap-aware splitting + token-budget guard for large symbol bodies.

2. **Generic file-level fallback chunker (new)**
   - When a file has no extracted symbols (or parser unsupported), generate fallback chunks directly from file text.
   - Use the same chunk-size/overlap/token-budget contract as symbol chunks.

3. **Unified chunk metadata contract**
   - Every chunk (symbol or fallback) carries origin metadata:
     - `origin = symbol | file_fallback`
     - stable file path + line range + truncated flag.

4. **Semantic retrieval compatibility**
   - Query pipeline treats both origins uniformly with origin-aware dedup and explain metadata.

## Capabilities

### New Capabilities
- `chunking-universal-coverage`: semantic chunk generation for both symbol-backed and fallback file-backed paths.

### Modified Capabilities
- `chunking-quality`: now includes no-symbol fallback behavior.

## Impact

- Affected crates: `cruxe-indexer`, `cruxe-state`, `cruxe-query`, `cruxe-core`.
- API impact: no MCP parameter changes required.
- Data impact: vector count may increase due to fallback chunks; bounded by existing semantic budgets.
- Product impact: semantic retrieval remains useful even in partially supported languages.
