# MCP Tool Contracts: Semantic/Hybrid Search

Transport: JSON-RPC 2.0 over stdio (v1).

All responses include Protocol v1 metadata fields.
Error codes MUST follow the canonical registry in `specs/meta/protocol-error-codes.md`.

This spec does not add new MCP tools. It extends the existing `search_code` tool with
semantic search parameters and enhanced response metadata.

## Protocol v1 Response Metadata (Extended)

Included in every search response when semantic search is available:

```json
{
  "metadata": {
    "codecompass_protocol_version": "1.0",
    "freshness_status": "fresh | stale | syncing",
    "indexing_status": "not_indexed | indexing | ready | failed",
    "result_completeness": "complete | partial | truncated",
    "ref": "main",
    "semantic_mode": "off | rerank_only | hybrid",
    "semantic_enabled": true,
    "semantic_ratio_used": 0.3,
    "semantic_triggered": true,
    "semantic_skipped_reason": null,
    "external_provider_blocked": false,
    "embedding_model_version": "fastembed-1",
    "rerank_provider": "cohere | voyage | local | none",
    "rerank_fallback": false,
    "low_confidence": false,
    "suggested_action": null,
    "confidence_threshold": 0.5,
    "top_score": 0.82,
    "score_margin": 0.11,
    "channel_agreement": 0.67,
    "query_intent_confidence": 0.91,
    "intent_escalation_hint": null
  }
}
```

## Tool: `search_code` (Updated)

Search across symbols, snippets, and files with query intent classification.
Now supports optional semantic/hybrid search for natural language queries.

### Input (Extended)

```json
{
  "query": "where is rate limiting implemented",
  "ref": "main",
  "language": "rust",
  "limit": 10,
  "semantic_ratio": 0.5,
  "confidence_threshold": 0.4
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `query` | string | yes | Search query (symbol name, path, error string, or natural language). |
| `ref` | string | no | Branch/ref scope. |
| `language` | string | no | Filter by language. |
| `limit` | int | no | Max results. Default: 10. |
| `semantic_ratio` | float | no | Override semantic/lexical blend ratio cap (0.0-1.0). Runtime may reduce actual usage based on lexical confidence. Only applies when semantic is enabled and intent is `natural_language`. Default: from config. |
| `confidence_threshold` | float | no | Override confidence threshold for low-confidence detection. Default: from config (0.5). |

### Output (Extended)

```json
{
  "results": [
    {
      "path": "src/middleware/rate_limit.rs",
      "line_start": 15,
      "line_end": 48,
      "kind": "fn",
      "name": "check_rate_limit",
      "qualified_name": "middleware::rate_limit::check_rate_limit",
      "language": "rust",
      "score": 0.82,
      "snippet": "pub fn check_rate_limit(req: &Request) -> Result<()> { ... }",
      "provenance": "hybrid"
    }
  ],
  "query_intent": "natural_language",
  "total_candidates": 47,
  "metadata": {
    "codecompass_protocol_version": "1.0",
    "freshness_status": "fresh",
    "indexing_status": "ready",
    "result_completeness": "complete",
    "ref": "main",
    "semantic_mode": "hybrid",
    "semantic_enabled": true,
    "semantic_ratio_used": 0.5,
    "semantic_triggered": true,
    "semantic_skipped_reason": null,
    "external_provider_blocked": false,
    "embedding_model_version": "fastembed-1",
    "rerank_provider": "cohere",
    "rerank_fallback": false,
    "low_confidence": false,
    "suggested_action": null,
    "confidence_threshold": 0.4,
    "top_score": 0.82,
    "score_margin": 0.11,
    "channel_agreement": 0.67,
    "query_intent_confidence": 0.91,
    "intent_escalation_hint": null
  }
}
```

### New Response Fields

| Field | Type | Description |
|-------|------|-------------|
| `results[].provenance` | string | `"lexical"`, `"semantic"`, or `"hybrid"` indicating which search path produced this result. |
| `metadata.semantic_enabled` | bool | Whether semantic search was active for this query. |
| `metadata.semantic_mode` | string | Effective mode: `"off"`, `"rerank_only"`, `"hybrid"`. |
| `metadata.semantic_ratio_used` | float | The actual ratio used (may differ from request; runtime may reduce semantic weight based on lexical confidence). |
| `metadata.semantic_triggered` | bool | Whether semantic retrieval branch actually executed for this query. |
| `metadata.semantic_skipped_reason` | string/null | Why semantic retrieval was skipped (`intent_not_nl`, `lexical_high_confidence`, `semantic_disabled`, etc.). |
| `metadata.external_provider_blocked` | bool | `true` when external provider path is configured but blocked by privacy policy gates. |
| `metadata.embedding_model_version` | string | Embedding model/version used for this query, when semantic path is active. |
| `metadata.rerank_provider` | string | Which reranker was used: `"cohere"`, `"voyage"`, `"local"`, or `"none"`. |
| `metadata.rerank_fallback` | bool | `true` if external reranker failed and local was used instead. |
| `metadata.low_confidence` | bool | `true` if composite confidence is below the confidence threshold. |
| `metadata.suggested_action` | string/null | Suggested next action when `low_confidence` is true. Null otherwise. |
| `metadata.confidence_threshold` | float | The threshold used for low-confidence detection. |
| `metadata.top_score` | float | Top result score used in composite confidence evaluation. |
| `metadata.score_margin` | float | Difference between top1 and top2 scores used in composite confidence evaluation. |
| `metadata.channel_agreement` | float | Agreement signal between lexical and semantic channels (0.0-1.0). |
| `metadata.query_intent_confidence` | float | Confidence of intent classification (0.0-1.0). |
| `metadata.intent_escalation_hint` | string/null | Agent hint when intent confidence is low (for example retry intent as symbol/path/NL). |
| `metadata.semantic_fallback` | bool | `true` when the embedding model was unavailable and results are purely lexical. |

### Semantic Search Behavior by Intent

| Intent | Semantic Used | Rationale |
|--------|--------------|-----------|
| `natural_language` | Yes (only in `hybrid` mode and not short-circuited) | NL queries benefit most from semantic similarity |
| `symbol` | No | Symbol lookup is precise; lexical is better |
| `path` | No | Path queries are exact; no benefit from semantic |
| `error` | No | Error strings need exact matching |

`semantic_mode = rerank_only` keeps this table lexical for all intents and only
enables rerank behavior on lexical candidates.

### Low-Confidence Suggested Actions

| Condition | Suggested Action |
|-----------|-----------------|
| NL intent, composite confidence < threshold | `"Try locate_symbol with '{extracted_identifier}'"` |
| Symbol intent, 0 results | `"Try search_code with broader query: '{original_query}'"` |
| Symbol intent, low score | `"Try search_code with natural language: 'where is {name} defined'"` |
| Path intent, 0 results | `"Check file path spelling or try search_code with filename"` |
| Any intent, 0 results | `"No results found. Try broader search terms or check index status."` |

### Errors (No New Errors)

Existing error codes apply. Semantic search failures are handled transparently
(fallback to lexical) and do not produce new error codes.

---

## Configuration Reference

### config.toml `[semantic]` Section

```toml
[semantic]
# Semantic mode:
# - off: lexical only
# - rerank_only: lexical + optional rerank, no embeddings/vector index
# - hybrid: lexical + vector hybrid + optional rerank
mode = "off"

# Max semantic blend ratio for natural language queries (runtime may reduce this value)
ratio = 0.3

# If lexical confidence exceeds this threshold, semantic retrieval is skipped
lexical_short_circuit_threshold = 0.85

# Confidence threshold for low-confidence detection
confidence_threshold = 0.5

# Profile advisor mode:
# - off: do not emit profile recommendation
# - suggest: emit recommendation in diagnostics/status output
profile_advisor_mode = "off"

# Global policy gates for outbound provider calls (secure-by-default)
external_provider_enabled = false
allow_code_payload_to_external = false

[semantic.embedding]
# Embedding profile: "fast_local", "code_quality", "high_quality", or "external"
profile = "fast_local"

# Embedding provider: "local", "voyage", or "openai"
provider = "local"

# Model name (for local: fastembed model ID, for API: model identifier)
model = "NomicEmbedTextV15Q"

# Embedding model version (used for vector compatibility partitioning)
model_version = "fastembed-1"

# Embedding dimensions (must match model output)
dimensions = 768

# Batch size for embedding generation during indexing
batch_size = 32

[semantic.rerank]
# Rerank provider: "none", "cohere", "voyage"
provider = "none"

# Timeout for external rerank API calls
timeout_ms = 5000

# Per-query-type overrides
[semantic.overrides.natural_language]
ratio = 0.5

[semantic.overrides.symbol]
ratio = 0.0

[semantic.overrides.path]
ratio = 0.0

[semantic.overrides.error]
ratio = 0.0
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `CODECOMPASS_EMBEDDING_API_KEY` | API key for external embedding provider (Voyage, OpenAI). Never logged. |
| `CODECOMPASS_RERANK_API_KEY` | API key for external rerank provider (Cohere, Voyage). Never logged. |
