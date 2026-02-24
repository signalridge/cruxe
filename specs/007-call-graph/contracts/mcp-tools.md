# MCP Tool Contracts: Call Graph Analysis

Transport: JSON-RPC 2.0 over stdio (v1).

All responses include Protocol v1 metadata fields.
Error codes MUST follow the canonical registry in `specs/meta/protocol-error-codes.md`.

## Protocol v1 Response Metadata

Included in every tool response:

```json
{
  "metadata": {
    "codecompass_protocol_version": "1.0",
    "freshness_status": "fresh | stale | syncing",
    "indexing_status": "not_indexed | indexing | ready | failed",
    "result_completeness": "complete | partial | truncated",
    "ref": "main",
    "schema_status": "compatible | not_indexed | reindex_required | corrupt_manifest"
  }
}
```

## Tool: `get_call_graph`

Return callers and/or callees for a given symbol, traversing the call graph to a
configurable depth. Enables impact analysis and dependency understanding.

### Input

```json
{
  "symbol_name": "validate_token",
  "path": "src/auth/jwt.rs",
  "ref": "main",
  "direction": "both",
  "depth": 1,
  "limit": 20
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `symbol_name` | string | yes | Name of the symbol to query. |
| `path` | string | no | File path to disambiguate symbols with the same name. |
| `ref` | string | no | Branch/ref scope. Default: current HEAD or `"live"`. |
| `direction` | string | no | `"callers"`, `"callees"`, or `"both"`. Default: `"both"`. |
| `depth` | int | no | Traversal depth (1-5). Default: 1. Capped at 5. |
| `limit` | int | no | Max results per direction. Default: 20. |

### Output

```json
{
  "symbol": {
    "symbol_id": "sym_01HQ6V0B2Q7K6D7A2GP9R4P3ME",
    "symbol_stable_id": "b3:1e0f98...",
    "name": "validate_token",
    "qualified_name": "auth::jwt::validate_token",
    "path": "src/auth/jwt.rs",
    "line_start": 87,
    "line_end": 112,
    "kind": "fn"
  },
  "callers": [
    {
      "symbol": {
        "symbol_id": "sym_01HQ6V0F6YBHHQ8YJ5QSW2J9C2",
        "symbol_stable_id": "b3:79ce4a...",
        "name": "authenticate",
        "qualified_name": "auth::middleware::authenticate",
        "path": "src/auth/middleware.rs",
        "line_start": 23,
        "line_end": 45,
        "kind": "fn"
      },
      "call_site": {
        "file": "src/auth/middleware.rs",
        "line": 34
      },
      "confidence": "static",
      "depth": 1
    }
  ],
  "callees": [
    {
      "symbol": {
        "symbol_id": "sym_01HQ6V0G2EGF5J5H8EWXMW0W6N",
        "symbol_stable_id": "b3:c4a22b...",
        "name": "decode_jwt",
        "qualified_name": "auth::jwt::decode_jwt",
        "path": "src/auth/jwt.rs",
        "line_start": 120,
        "line_end": 145,
        "kind": "fn"
      },
      "call_site": {
        "file": "src/auth/jwt.rs",
        "line": 95
      },
      "confidence": "static",
      "depth": 1
    }
  ],
  "total_edges": 2,
  "truncated": false,
  "metadata": { ... }
}
```

### Errors

| Code | Meaning |
|------|---------|
| `symbol_not_found` | No symbol matching the given name (and optional path) in the specified ref. |
| `ref_not_indexed` | The specified ref has not been indexed. |

Depth values above max (5) are clamped to 5 and reported via `metadata.warnings[]`
as a non-error warning (no protocol error envelope).

---

## Tool: `compare_symbol_between_commits`

Show how a symbol changed between two commits/refs. Returns diff of signature, body,
and line range.

### Input

```json
{
  "symbol_name": "process_request",
  "path": "src/handler.rs",
  "base_ref": "main",
  "head_ref": "feat/auth"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `symbol_name` | string | yes | Name of the symbol to compare. |
| `path` | string | no | File path to disambiguate. |
| `base_ref` | string | yes | Base ref (commit, branch, tag). |
| `head_ref` | string | yes | Head ref (commit, branch, tag). |

### Output

```json
{
  "symbol": "process_request",
  "path": "src/handler.rs",
  "symbol_stable_id": "b3:4f3d0c...",
  "base_version": {
    "symbol_id": "sym_01HQ6V15Q2WVY45F8M0P1TZM9T",
    "signature": "pub fn process_request(req: &Request) -> Response",
    "line_start": 45,
    "line_end": 78,
    "kind": "fn",
    "language": "rust"
  },
  "head_version": {
    "symbol_id": "sym_01HQ6V15Q2WVY45F8M0P1TZM9T",
    "signature": "pub async fn process_request(req: &Request, ctx: &Context) -> Result<Response>",
    "line_start": 45,
    "line_end": 92,
    "kind": "fn",
    "language": "rust"
  },
  "diff_summary": {
    "signature_changed": true,
    "body_changed": true,
    "lines_added": 18,
    "lines_removed": 4,
    "line_range_shifted": true
  },
  "metadata": { ... }
}
```

### Special Cases

- **Symbol added in head_ref**: `base_version` is `null`, `diff_summary.status` is `"added"`.
- **Symbol deleted in head_ref**: `head_version` is `null`, `diff_summary.status` is `"deleted"`.
- **Symbol unchanged**: `diff_summary` is `{ "status": "unchanged" }`.

### Errors

| Code | Meaning |
|------|---------|
| `symbol_not_found` | Symbol not found in either ref. |
| `ref_not_indexed` | One or both refs have not been indexed. |

---

## Tool: `suggest_followup_queries`

Analyze previous query results and suggest next tool calls when confidence is low.
Designed for AI agent self-correction workflows.

### Input

```json
{
  "previous_query": {
    "tool": "search_code",
    "params": {
      "query": "where is rate limiting implemented"
    }
  },
  "previous_results": {
    "results": [...],
    "query_intent": "natural_language",
    "total_candidates": 3,
    "top_score": 0.25
  },
  "confidence_threshold": 0.5
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `previous_query` | object | yes | The tool name and params of the previous query. |
| `previous_results` | object | yes | The results from the previous query (can be abbreviated). |
| `confidence_threshold` | float | no | Score below which results are considered low-confidence. Default: 0.5. |

### Output

```json
{
  "suggestions": [
    {
      "tool": "locate_symbol",
      "params": {
        "name": "rate_limit",
        "kind": "fn"
      },
      "reason": "Extracted identifier 'rate_limit' from natural language query. Symbol lookup may yield more precise results."
    },
    {
      "tool": "search_code",
      "params": {
        "query": "rate_limit middleware",
        "language": "rust"
      },
      "reason": "Narrowing query to specific terms and language may improve relevance."
    }
  ],
  "analysis": {
    "previous_confidence": "low",
    "top_score": 0.25,
    "threshold": 0.5,
    "extracted_identifiers": ["rate_limit", "rate_limiting"]
  },
  "metadata": { ... }
}
```

### Suggestion Rules

| Previous Tool | Condition | Suggested Action |
|---------------|-----------|-----------------|
| `search_code` | top_score < threshold, intent=natural_language | Suggest `locate_symbol` with extracted identifiers |
| `search_code` | top_score < threshold, intent=symbol | Suggest `search_code` with broader query or different language |
| `locate_symbol` | 0 results | Suggest `search_code` with symbol name; suggest `get_call_graph` if symbol might be callee |
| `get_call_graph` | 0 edges | Suggest `locate_symbol` to verify symbol exists; suggest `search_code` for alternative names |
| any | top_score >= threshold | Empty suggestions with reason "results are above confidence threshold" |

### Errors

| Code | Meaning |
|------|---------|
| `invalid_input` | The `previous_query` object is missing required fields. |
