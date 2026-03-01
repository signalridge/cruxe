## Why

Cruxe currently deduplicates at symbol level but can still return top-k results concentrated in one file. This hurts exploration and agent workflows, especially in large repositories.

Diversity should be improved conservatively and language-agnostically:

- prevent pathological clustering,
- preserve relevance ordering,
- avoid per-language heuristics.

## What Changes

1. **Conservative file-spread diversity pass**
   - Apply after ranking and before truncation.
   - Use bounded sliding-window constraints.

2. **Optional per-request control**
   - Keep `diversity: bool` in `search_code` (default true).

3. **Score-floor safeguard**
   - Never promote low-quality candidates solely for diversity.

4. **Benchmark gate**
   - Track diversity gain and relevance regression together.

## Capabilities

### New Capabilities
- `result-diversity`: language-agnostic post-rank file-spread balancing.

### Modified Capabilities
- `002-agent-protocol`: optional `diversity` parameter for `search_code`.

## Impact

- Affected crates: `cruxe-query`, `cruxe-mcp`.
- Data impact: none.
- Runtime impact: low, bounded post-processing complexity.
