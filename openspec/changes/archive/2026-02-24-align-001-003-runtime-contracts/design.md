## Context

`001-core-mvp`, `002-agent-protocol`, and `003-structure-nav` now define a
single protocol contract for agent-facing metadata and query behavior. The
current runtime is partially aligned, but still contains legacy output values
and feature toggles (for example `debug.ranking_reasons`) that do not match the
latest spec text.

This change is cross-cutting across `cruxe-core`, `cruxe-query`,
`cruxe-mcp`, and `cruxe-cli`, and directly affects MCP tool
contracts consumed by external AI tools. That makes compatibility, migration,
and deterministic behavior more important than isolated local refactors.

## Goals / Non-Goals

**Goals:**

- Align runtime outputs to canonical enums required by 001-003:
  - `indexing_status`: `not_indexed | indexing | ready | failed`
  - `result_completeness`: `complete | partial | truncated`
- Implement 002 protocol requirements that were documented but not fully
  enforced in runtime:
  - `compact` input behavior (serialization-time shaping)
  - `ranking_explain_level` (`off|basic|full`)
  - FR-105b duplicate suppression metadata
  - FR-105c payload safety limits with graceful truncation semantics
- Preserve backward compatibility for pre-migration clients/configs where
  practical.
- Add/adjust tests so spec-to-runtime drift is caught by CI.

**Non-Goals:**

- No changes to storage schema or index format.
- No introduction of external dependencies.
- No expansion of `compact` to 003 tools in this change (003 remains token
  budget-first without a dedicated `compact` input).
- No semantic retrieval model changes (008 scope).

## Decisions

### 1) Canonical enum migration uses “canonical write + legacy read” strategy

Runtime types will serialize only canonical enum values. Deserialization and
compatibility shims will accept legacy aliases where needed (`idle` → `ready`,
`partial_available` → `ready`) as safe defaults for stale cached
payloads/tests. Legacy `Idle` was overloaded across healthy, not-indexed, and
error states; the primary fix is at the builder layer (see Decision 2), not
the deserialization alias. `not_indexed` and `failed` are new variants with no
legacy alias.

Why this over dual-write:

- Dual-write increases ambiguity and long-tail migration burden.
- Canonical-only output makes downstream agent policy simpler and deterministic.

### 2) Metadata emission remains centralized in MCP protocol builders

`ProtocolMetadata` and related builder paths (`build_metadata*`) stay as the
single place that maps runtime state to protocol status fields. Individual tool
handlers only set deltas (for example marking `truncated` when payload limits
apply).

Builder-to-canonical mapping (legacy `Idle` was overloaded):

| Builder method        | `indexing_status` | `result_completeness` |
|-----------------------|-------------------|-----------------------|
| `new()`               | `ready`           | `complete`            |
| `not_indexed()`       | `not_indexed`     | `partial`             |
| `syncing()`           | `indexing`        | `partial`             |
| `reindex_required()`  | `failed`          | `partial`             |
| `corrupt_manifest()`  | `failed`          | `partial`             |

`schema_status` remains the compatibility gate for queryability, but for
runtime metadata semantics both `reindex_required()` and `corrupt_manifest()`
represent unusable index state and therefore map to `indexing_status: failed`.

Why:

- Avoids drift where each tool re-implements status mapping.
- Keeps compatibility and migration behavior testable in one place.

### 3) Explainability control moves to `ranking_explain_level` with strict precedence

Precedence:

1. per-request `ranking_explain_level` (if provided)
2. config default `search.ranking_explain_level`
3. legacy `debug.ranking_reasons` fallback (`true` → `full`, `false` → `off`)

`off` emits no reasons, `basic` emits compact normalized factors, `full` emits
current detailed factors.

Why:

- Matches 002 contracts exactly.
- Supports agent routing and human debugging without forcing full payload cost.

Alternative considered: keep only global config flag. Rejected because it cannot
support per-call policy and conflicts with 002 contract.

### 4) `compact` remains a serialization concern, not a retrieval concern

`compact` is applied after retrieval/ranking and `detail_level` shaping. It
removes large optional fields while preserving identity/location/score and
deterministic follow-up handles.

Why:

- Keeps ranking logic stable.
- Minimizes behavioral regressions while meeting token/payload goals.

### 5) FR-105b dedup happens before final output assembly with explicit metadata

Near-identical hits are deduped by symbol/file-region identity key before final
emission. The response includes `suppressed_duplicate_count` for observability.

Why:

- Avoids repetitive payload with low agent value.
- Makes improvements measurable without hiding that suppression occurred.

### 6) FR-105c safety limit uses graceful truncation

Query tools enforce a hard response payload budget. When exceeded:

- keep deterministic prefix of shaped results,
- set `result_completeness: "truncated"`,
- set `safety_limit_applied: true`,
- return deterministic `suggested_next_actions` instead of hard failure.

Why:

- Keeps tool usable under strict context limits.
- Matches “fail-soft” protocol intent in 002.

Interaction with existing 003 truncation: 003 tools already implement
token-budget truncation at the query layer (for example `get_code_context`
breaks when estimated tokens exceed `max_tokens`). The FR-105c safety limit
acts as an outer envelope at response serialization. If the query layer has
already set `result_completeness: "truncated"`, the safety-limit check respects
this and does not re-truncate the already-bounded result.

## Risks / Trade-offs

- **[Compatibility confusion during migration]** Different clients may assume
  old status values. → Mitigation: legacy alias read-path + compatibility tests
  + changelog note.
- **[Payload-size guards may drop useful context]** Aggressive limits can reduce
  answer quality. → Mitigation: deterministic follow-up suggestions and
  configurable limits in subsequent iteration.
- **[Explainability overhead]** `full` reasons can increase latency/bytes. →
  Mitigation: default `off`, `basic` middle mode, benchmark gate for overhead.
- **[Dedup false positives]** Overly coarse dedup keys may hide meaningful
  variants. → Mitigation: start with conservative identity key and add fixtures
  for edge cases.

## Migration Plan

1. Update core enums/config parsing with compatibility handling.
2. Align MCP protocol metadata defaults/mappings to canonical values.
3. Wire `ranking_explain_level` input and config precedence.
4. Apply `compact` shaping and dedup/safety-limit behavior in query tool
   response assembly.
5. Add/update tests for enum serialization, compatibility aliases, explainability
   levels, dedup counts, and truncation metadata.
6. Run `cargo test --workspace`, `cargo clippy --workspace`, and `cargo fmt
   --check` before merge.

Rollback strategy:

- Revert the change set and keep legacy behavior (`debug.ranking_reasons` +
  prior status mapping) if downstream integrations fail unexpectedly.

## Resolved Questions

- **FR-105c payload budget**: Hard-code a default (64 KB) for now. Reserve a
  `search.max_response_bytes` config field with `#[serde(default)]` so it can
  be exposed without a breaking change later.
- **`basic` factor names**: Standardize now to
  `exact_match | path_boost | definition_boost | semantic_similarity | final_score`.
  Current internal factor structure is close enough that mapping is
  straightforward, and deferring would create a later breaking change.
- **`suppressed_duplicate_count`**: Emit only when non-zero
  (`#[serde(skip_serializing_if = "is_zero")]`). Zero carries no information;
  telemetry consumers can treat a missing field as zero.
