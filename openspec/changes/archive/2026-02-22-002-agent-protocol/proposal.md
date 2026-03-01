## Why

AI coding agents calling Cruxe's MCP tools receive uniform response payloads regardless of query intent, wasting token budget on fields the agent does not need. Agents also lack operational visibility (health, freshness, ranking explanations) and suffer cold-start latency on first queries. Constitution principles V (Agent-Aware Response Design) and VII (Explainable Ranking) mandate detail-level control and transparent scoring as Phase 1.1 MUSTs.

## What Changes

1. Add `detail_level` parameter (`location` / `signature` / `context`) and `compact` mode to `search_code` and `locate_symbol` for agent-controlled response verbosity.
2. Add `get_file_outline` MCP tool returning a nested symbol tree for a file path without full-file reads.
3. Add `health_check` MCP tool reporting Tantivy index health, SQLite integrity, grammar availability, active jobs, prewarm status, and startup compatibility.
4. Add Tantivy index prewarming on `serve-mcp` startup (async, non-blocking) with `--no-prewarm` opt-out.
5. Add `ranking_explain_level` (`off` / `basic` / `full`) exposing per-result scoring breakdowns in search metadata.
6. Add configurable `freshness_policy` (`strict` / `balanced` / `best_effort`) for stale-aware query behavior with per-request override.

## Capabilities

### New Capabilities

- **`get_file_outline`**: MCP tool returning nested symbol tree for a given file path and ref, with `depth` (`top` | `all`) and optional `language` parameters. Pure SQLite query against `symbol_relations` table, p95 < 50ms.
  - FR-106: MUST provide a `get_file_outline` MCP tool that returns a nested symbol tree for a given file path and ref.
  - FR-107: `get_file_outline` MUST accept `path`, `ref`, `depth` (`top` | `all`), and optional `language` parameters.
  - FR-108: `get_file_outline` MUST query the `symbol_relations` SQLite table using `parent_symbol_id` chains to build nested trees, with p95 < 50ms.
  - FR-109: `get_file_outline` with `depth: "top"` MUST return only symbols where `parent_symbol_id IS NULL`.
- **`health_check`**: MCP tool reporting operational status including Tantivy index health, SQLite integrity, grammar availability, active indexing job status, prewarm status, and startup compatibility payload (`index.status`, `current_schema_version`, `required_schema_version`).
- **Index prewarming**: On `serve-mcp` startup, forces mmap pages into memory by touching segment metadata and running warmup queries. Runs asynchronously after MCP handshake acceptance. `--no-prewarm` CLI flag to disable.

### Modified Capabilities

- **`search_code` / `locate_symbol`**:
  - FR-101: MUST accept `detail_level` parameter with values `"location"`, `"signature"` (default), `"context"`, controlling response field inclusion.
  - FR-102: At `"location"`: only `path`, `line_start`, `line_end`, `kind`, `name` (~50 tokens per result).
  - FR-103: At `"signature"`: additionally `qualified_name`, `signature`, `language`, `visibility` (~100 tokens).
  - FR-104: At `"context"`: additionally `body_preview` (first N lines, truncated), `parent` context (kind, name, path, line), `related_symbols` array (~300-500 tokens).
  - FR-105: `detail_level` applied to serialization only, not query/ranking logic.
  - FR-105a: MUST accept `compact: bool` (default false). When true, omits large optional payload fields while preserving identity/location/score/follow-up handles.
  - FR-105b: MUST deduplicate near-identical hits by symbol/file-region before final top-k emission; surface suppressed count in metadata.
  - FR-105c: MUST enforce hard payload safety limits with `result_completeness: "truncated"` and deterministic `suggested_next_actions` instead of hard failure.
  - FR-114: MUST include optional `ranking_reasons` field in metadata when `ranking_explain_level` is not `off`.
  - FR-115: `ranking_reasons` contains per-result breakdown: `exact_match_boost`, `qualified_name_boost`, `path_affinity`, `definition_boost`, `kind_match`, `bm25_score`.
  - FR-115a: MUST support `ranking_explain_level` (`off` | `basic` | `full`): `off` omits explanations, `basic` emits compact normalized factors, `full` emits full debug breakdown.
  - FR-116: MUST perform pre-query freshness check with configurable policy: `strict`, `balanced` (default), `best_effort`.
  - FR-117: Under `strict`, MUST block queries when stale and return error with guidance.
  - FR-118: Under `balanced`, MUST return results with stale indicator and trigger async sync.
  - FR-119: Under `best_effort`, MUST always return results immediately without sync.
  - FR-120: Freshness policy configurable in `config.toml` and overridable per-request.
- **`serve-mcp` startup**:
  - FR-111: MUST prewarm Tantivy indices by default (touch segment metadata, run warmup queries).
  - FR-112: MUST support `--no-prewarm` CLI flag.
  - FR-113: Health status reports `"warming"` during prewarm, transitions to `"ready"`.
  - FR-121: MCP handshake requests (`initialize`, `tools/list`) MUST succeed while prewarming is in progress.
- **`health_check` / `index_status`**:
  - FR-110: MUST report Tantivy health, SQLite integrity, grammar availability, active job status, prewarm status, startup compatibility.
  - FR-122: MUST include startup compatibility payload with `index.status`, `current_schema_version`, `required_schema_version`.
  - FR-123: When `reindex_required` or `corrupt_manifest`, query tools MUST return `index_incompatible` with remediation guidance.

### Key Entities

No new entities introduced. This spec extends existing entities from 001-core-mvp:

- **Symbol**: Extended with detail-level-aware serialization (location / signature / context response shapes).
- **Protocol v1 Metadata**: Extended with optional `ranking_reasons` field and refined `freshness_status` semantics via policy levels.

## Impact

### Success Criteria

- SC-101: `detail_level: "location"` responses <= 60 tokens per result on average.
- SC-102: `detail_level: "signature"` responses <= 120 tokens per result on average.
- SC-102a: `compact: true` responses <= 20% payload bytes vs non-compact for the same query/limit.
- SC-103: `get_file_outline` p95 < 50ms on files with up to 200 symbols.
- SC-104: First query after `serve-mcp` startup (with prewarm) p95 < 500ms.
- SC-105: `health_check` p95 < 10ms.
- SC-106: Stale-aware `balanced` policy returns results within same p95 latency envelope (freshness check adds < 5ms).
- SC-107: `ranking_explain_level: "basic"` increases warm `search_code` p95 latency by <= 10% versus `off`.

### Edge Cases

- `detail_level: "context"` with no parent or related symbols: `parent` and `related_symbols` fields are omitted (not null); `body_preview` still included if available.
- `get_file_outline` on file with no symbols (e.g., config file without tree-sitter grammar): empty `symbols` array returned with file metadata.
- Prewarming fails (e.g., corrupted index segment): health status reports `"error"` with diagnostics; queries still accepted but fall back to cold-start behavior.
- `ranking_reasons` requested on `locate_symbol`: included, since `locate_symbol` also uses the ranking pipeline.
- Freshness check itself is slow (large repo): uses lightweight signals (HEAD commit comparison for VCS, manifest hash cursor for single-version); does not scan files.
- Index schema incompatible after upgrade: `health_check` and `index_status` expose `startup_checks.index.status = reindex_required`; query tools return actionable `index_incompatible` errors until `cruxe index --force` completes.

### Affected Crates

`cruxe-core`, `cruxe-query`, `cruxe-state`, `cruxe-mcp`, `cruxe-cli`.

### API Impact

All changes are backward-compatible with Protocol v1 response contract. Existing clients calling `search_code` and `locate_symbol` without `detail_level` receive `"signature"` level responses (default).

### Performance Impact

- `get_file_outline`: pure SQLite query, p95 < 50ms.
- `health_check`: p95 < 10ms.
- Prewarm eliminates cold-start penalty (first query p95 < 500ms vs > 2000ms without).
- `ranking_explain_level: "basic"` adds <= 10% latency overhead.
- Freshness check adds < 5ms to query path.

### Implementation Alignment Notes (2026-02-25)

- MCP query handlers are decomposed by domain (`query/structure/context/index/health/status`) with shared helper modules for freshness, detail filtering, dedup, and payload limits.
- `ranking_explain_level` and `freshness_policy` config values are normalized to canonical runtime enums at load time, with legacy compatibility preserved.
- Strict p95 assertions were moved into benchmark-harness entry points; default CI keeps smoke-level latency guards.
