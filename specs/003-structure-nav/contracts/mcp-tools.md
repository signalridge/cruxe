# MCP Tool Contracts: Symbol Structure & Navigation

Transport: JSON-RPC 2.0 over stdio (v1). Extends the tool surface from 001-core-mvp.

All responses include Protocol v1 metadata fields (defined in 001-core-mvp contracts).
Error codes MUST follow the canonical registry in `specs/meta/protocol-error-codes.md`.

## Compact Flag Scope

`003` tools in this contract (`get_symbol_hierarchy`, `find_related_symbols`,
`get_code_context`) do not define a dedicated `compact` input parameter.
Token-size control is handled by `max_tokens` + strategy shaping in this phase.
`compact` remains explicitly scoped to `search_code`/`locate_symbol` from `002`.

## Protocol v1 Response Metadata (inherited)

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

---

## Tool: `get_symbol_hierarchy`

Traverse the parent chain (ancestors) or child tree (descendants) for a given
symbol. Uses `parent_symbol_id` in `symbol_relations` for structural navigation.

### Input

```json
{
  "symbol_name": "validate_token",
  "path": "src/auth/jwt.rs",
  "ref": "main",
  "direction": "ancestors"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `symbol_name` | string | yes | Name of the symbol to start from. |
| `path` | string | no | File path to disambiguate symbols with the same name. If omitted and matches span multiple files, returns `ambiguous_symbol`. |
| `ref` | string | no | Branch/ref scope. Default: current HEAD or `"live"`. |
| `direction` | string | no | `"ancestors"` (leaf to root) or `"descendants"` (root to leaves). Default: `"ancestors"`. |

### Output (ancestors)

```json
{
  "hierarchy": [
    {
      "symbol_id": "sym_01HQ6XH4E2D9E8M8SN8C1A2R7F",
      "symbol_stable_id": "b3:7d2a6f0f8f...",
      "name": "validate_token",
      "kind": "fn",
      "qualified_name": "auth::jwt::validate_token",
      "path": "src/auth/jwt.rs",
      "line_start": 87,
      "line_end": 112,
      "signature": "pub fn validate_token(token: &str, key: &[u8]) -> Result<Claims>",
      "depth": 0
    },
    {
      "symbol_id": "sym_01HQ6XH7ATF5M9R7X2W8K01JHN",
      "symbol_stable_id": "b3:58af10...",
      "name": "JwtValidator",
      "kind": "impl",
      "qualified_name": "auth::jwt::JwtValidator",
      "path": "src/auth/jwt.rs",
      "line_start": 45,
      "line_end": 120,
      "signature": "impl JwtValidator",
      "depth": 1
    }
  ],
  "direction": "ancestors",
  "chain_length": 2,
  "metadata": { ... }
}
```

### Output (descendants)

```json
{
  "hierarchy": [
    {
      "symbol_id": "sym_01HQ6XH7ATF5M9R7X2W8K01JHN",
      "symbol_stable_id": "b3:58af10...",
      "name": "JwtValidator",
      "kind": "impl",
      "qualified_name": "auth::jwt::JwtValidator",
      "path": "src/auth/jwt.rs",
      "line_start": 45,
      "line_end": 120,
      "depth": 0,
      "children": [
        {
          "symbol_id": "sym_01HQ6XH9T0SP8DA5Y1ET7Y0F8N",
          "symbol_stable_id": "b3:11ee78...",
          "name": "new",
          "kind": "fn",
          "qualified_name": "auth::jwt::JwtValidator::new",
          "path": "src/auth/jwt.rs",
          "line_start": 47,
          "line_end": 55,
          "signature": "pub fn new(config: JwtConfig) -> Self",
          "depth": 1
        },
        {
          "symbol_id": "sym_01HQ6XH4E2D9E8M8SN8C1A2R7F",
          "symbol_stable_id": "b3:7d2a6f0f8f...",
          "name": "validate_token",
          "kind": "fn",
          "qualified_name": "auth::jwt::JwtValidator::validate_token",
          "path": "src/auth/jwt.rs",
          "line_start": 87,
          "line_end": 112,
          "signature": "pub fn validate_token(token: &str, key: &[u8]) -> Result<Claims>",
          "depth": 1
        }
      ]
    }
  ],
  "direction": "descendants",
  "chain_length": 3,
  "metadata": { ... }
}
```

### Errors

| Code | Meaning |
|------|---------|
| `symbol_not_found` | No symbol matching the name (and optional path) was found. |
| `ambiguous_symbol` | Multiple symbols match and no `path` was provided to disambiguate. |

---

## Tool: `find_related_symbols`

Find symbols in the same scope as a given symbol. Scope can be file-level,
module-level, or package-level. Uses `symbol_relations` for co-location and
`symbol_edges` for import graph connectivity.

### Input

```json
{
  "symbol_name": "validate_token",
  "path": "src/auth/jwt.rs",
  "ref": "main",
  "scope": "module",
  "limit": 20
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `symbol_name` | string | yes | Name of the anchor symbol. |
| `path` | string | no | File path to disambiguate. If omitted and matches span multiple files, returns `ambiguous_symbol`. |
| `ref` | string | no | Branch/ref scope. Default: current HEAD or `"live"`. |
| `scope` | string | no | `"file"`, `"module"`, or `"package"`. Default: `"file"`. |
| `limit` | int | no | Max results. Default: 20. |

### Output

```json
{
  "anchor": {
    "symbol_id": "sym_01HQ6XH4E2D9E8M8SN8C1A2R7F",
    "symbol_stable_id": "b3:7d2a6f0f8f...",
    "name": "validate_token",
    "kind": "fn",
    "path": "src/auth/jwt.rs",
    "line_start": 87
  },
  "related": [
    {
      "symbol_id": "sym_01HQ6XJ4N8GE6N6YPX5WWY8Q56",
      "symbol_stable_id": "b3:13ca77...",
      "name": "Claims",
      "kind": "struct",
      "qualified_name": "auth::jwt::Claims",
      "path": "src/auth/jwt.rs",
      "line_start": 12,
      "line_end": 25,
      "signature": "pub struct Claims { ... }",
      "relation": "same_file",
      "language": "rust"
    },
    {
      "symbol_id": "sym_01HQ6XJ8AS43JCF9R8MEV0FJ4D",
      "symbol_stable_id": "b3:8c99c4...",
      "name": "AuthHandler",
      "kind": "struct",
      "qualified_name": "auth::handler::AuthHandler",
      "path": "src/auth/handler.rs",
      "line_start": 10,
      "line_end": 18,
      "signature": "pub struct AuthHandler { ... }",
      "relation": "same_module",
      "language": "rust"
    },
    {
      "symbol_id": "sym_01HQ6XJA1H5SJX0Q9XJBQW7HZK",
      "symbol_stable_id": "b3:3af0dd...",
      "name": "TokenError",
      "kind": "enum",
      "qualified_name": "auth::error::TokenError",
      "path": "src/auth/error.rs",
      "line_start": 5,
      "line_end": 15,
      "signature": "pub enum TokenError { ... }",
      "relation": "imported",
      "language": "rust"
    }
  ],
  "scope_used": "module",
  "total_found": 3,
  "metadata": { ... }
}
```

### Relation Types in Results

| Relation | Meaning |
|----------|---------|
| `same_file` | Symbol is in the same source file. |
| `same_module` | Symbol is in a sibling file within the same module/package. |
| `same_package` | Symbol is in the same package scope but outside the immediate module. |
| `imported` | Symbol is connected via an import edge in `symbol_edges`. |

### Errors

| Code | Meaning |
|------|---------|
| `symbol_not_found` | No symbol matching the name (and optional path) was found. |
| `ambiguous_symbol` | Multiple symbols match and no `path` was provided to disambiguate. |

---

## Tool: `get_code_context`

Retrieve code context fitted to a token budget. Supports breadth (more symbols,
less detail) and depth (fewer symbols, more detail including body) strategies.

This tool fulfills Constitution Principle V (Agent-Aware Response Design) by
enabling agents to manage their context window explicitly.

### Input

```json
{
  "query": "how does authentication work",
  "max_tokens": 4000,
  "strategy": "breadth",
  "ref": "main",
  "language": "rust"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `query` | string | yes | Search query (same as `search_code`). |
| `max_tokens` | int | no | Maximum estimated tokens in response. Default: 4000. |
| `strategy` | string | no | `"breadth"` (many symbols, signature detail) or `"depth"` (fewer symbols, includes body). Default: `"breadth"`. |
| `ref` | string | no | Branch/ref scope. Default: current HEAD or `"live"`. |
| `language` | string | no | Filter by language. |

### Output (breadth strategy)

```json
{
  "context_items": [
    {
      "symbol_id": "sym_01HQ6XJ8AS43JCF9R8MEV0FJ4D",
      "symbol_stable_id": "b3:8c99c4...",
      "name": "authenticate",
      "kind": "fn",
      "qualified_name": "auth::handler::authenticate",
      "path": "src/auth/handler.rs",
      "line_start": 35,
      "line_end": 67,
      "signature": "pub fn authenticate(&self, req: &Request) -> Result<User>",
      "language": "rust",
      "score": 0.92
    },
    {
      "symbol_id": "sym_01HQ6XH4E2D9E8M8SN8C1A2R7F",
      "symbol_stable_id": "b3:7d2a6f0f8f...",
      "name": "validate_token",
      "kind": "fn",
      "qualified_name": "auth::jwt::validate_token",
      "path": "src/auth/jwt.rs",
      "line_start": 87,
      "line_end": 112,
      "signature": "pub fn validate_token(token: &str, key: &[u8]) -> Result<Claims>",
      "language": "rust",
      "score": 0.88
    },
    {
      "symbol_id": "sym_01HQ6XJC4QGRN4S8SJDGJ9Q0RE",
      "symbol_stable_id": "b3:ca0193...",
      "name": "AuthConfig",
      "kind": "struct",
      "qualified_name": "auth::config::AuthConfig",
      "path": "src/auth/config.rs",
      "line_start": 5,
      "line_end": 15,
      "signature": "pub struct AuthConfig { ... }",
      "language": "rust",
      "score": 0.75
    }
  ],
  "estimated_tokens": 387,
  "truncated": false,
  "metadata": {
    "total_candidates": 12,
    "returned": 3,
    "strategy": "breadth",
    "codecompass_protocol_version": "1.0",
    "freshness_status": "fresh",
    "indexing_status": "ready",
    "result_completeness": "complete",
    "ref": "main"
  }
}
```

### Output (depth strategy)

```json
{
  "context_items": [
    {
      "symbol_id": "sym_01HQ6XJ8AS43JCF9R8MEV0FJ4D",
      "symbol_stable_id": "b3:8c99c4...",
      "name": "authenticate",
      "kind": "fn",
      "qualified_name": "auth::handler::authenticate",
      "path": "src/auth/handler.rs",
      "line_start": 35,
      "line_end": 67,
      "signature": "pub fn authenticate(&self, req: &Request) -> Result<User>",
      "language": "rust",
      "score": 0.92,
      "body": "pub fn authenticate(&self, req: &Request) -> Result<User> {\n    let token = req.headers().get(\"Authorization\")\n        .ok_or(AuthError::MissingToken)?;\n    let claims = self.jwt.validate_token(token, &self.key)?;\n    self.user_store.find_by_id(claims.sub)\n}"
    }
  ],
  "estimated_tokens": 156,
  "truncated": false,
  "metadata": {
    "total_candidates": 12,
    "returned": 1,
    "strategy": "depth",
    "codecompass_protocol_version": "1.0",
    "freshness_status": "fresh",
    "indexing_status": "ready",
    "result_completeness": "complete",
    "ref": "main"
  }
}
```

### Output (truncated)

When the token budget is reached before all candidates are included:

```json
{
  "context_items": [ ... ],
  "estimated_tokens": 3842,
  "truncated": true,
  "metadata": {
    "total_candidates": 47,
    "returned": 12,
    "remaining_candidates": 35,
    "strategy": "breadth",
    "suggestion": "Use locate_symbol for specific symbols, or increase max_tokens",
    "codecompass_protocol_version": "1.0",
    "freshness_status": "fresh",
    "indexing_status": "ready",
    "result_completeness": "partial",
    "ref": "main"
  }
}
```

### Token Estimation Method

Token count is estimated per serialized context item:

```
estimated_tokens = ceil(whitespace_split_word_count * 1.3)
```

Where `whitespace_split_word_count` is the number of whitespace-delimited tokens
in the JSON-serialized form of the context item. The 1.3 multiplier is a
conservative approximation accounting for subword tokenization of code identifiers.

The response `estimated_tokens` field is the sum of all included items.

### Errors

| Code | Meaning |
|------|---------|
| `invalid_strategy` | Strategy is not `"breadth"` or `"depth"`. |
| `invalid_max_tokens` | `max_tokens` is less than 1. |

---

## Updated `tools/list` Response

After this feature, the MCP server's `tools/list` response includes these
additional tools (in addition to the 5 from 001-core-mvp and tools from
002-agent-protocol):

```json
[
  {
    "name": "get_symbol_hierarchy",
    "description": "Traverse the parent chain (ancestors) or child tree (descendants) for a symbol. Returns structural context: method -> class -> module.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "symbol_name": { "type": "string", "description": "Symbol name to start from" },
        "path": { "type": "string", "description": "File path to disambiguate; omitted may return ambiguous_symbol if multiple files match" },
        "ref": { "type": "string", "description": "Branch/ref scope" },
        "direction": { "type": "string", "enum": ["ancestors", "descendants"], "default": "ancestors" }
      },
      "required": ["symbol_name"]
    }
  },
  {
    "name": "find_related_symbols",
    "description": "Find symbols in the same file, module, or package scope as a given symbol. Uses symbol relations and import graph.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "symbol_name": { "type": "string", "description": "Anchor symbol name" },
        "path": { "type": "string", "description": "File path to disambiguate; omitted may return ambiguous_symbol if multiple files match" },
        "ref": { "type": "string", "description": "Branch/ref scope" },
        "scope": { "type": "string", "enum": ["file", "module", "package"], "default": "file" },
        "limit": { "type": "integer", "default": 20, "description": "Max results" }
      },
      "required": ["symbol_name"]
    }
  },
  {
    "name": "get_code_context",
    "description": "Retrieve code context fitted to a token budget. Use 'breadth' for many symbols with signatures, 'depth' for fewer symbols with full bodies.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "query": { "type": "string", "description": "Search query" },
        "max_tokens": { "type": "integer", "default": 4000, "description": "Token budget" },
        "strategy": { "type": "string", "enum": ["breadth", "depth"], "default": "breadth" },
        "ref": { "type": "string", "description": "Branch/ref scope" },
        "language": { "type": "string", "description": "Filter by language" }
      },
      "required": ["query"]
    }
  }
]
```
