# Adaptive Query Plan Evidence

Date: 2026-02-28

## Verification commands executed

### 1) Targeted adaptive plan + protocol tests

```bash
cargo test -p cruxe-query --test adaptive_query_plan_router_fixtures -- --nocapture
cargo test -p cruxe-mcp t47 -- --nocapture
```

Result: **PASS**

- Router fixture suites passed (Haystack-style + LlamaIndex-style).
- Retrieval-eval baseline comparison passed.
- Plan p95 budget and downgrade-rate assertions passed.
- MCP metadata presence/absence tests passed.
- Added protocol-level coverage for `query_plan_executed`.

### 2) Workspace clippy gate

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Result: **PASS**

### 3) Workspace test gate

Initial run:

```bash
cargo test --workspace
```

Result: **1 known perf test failure** in `cruxe-indexer`:

- `sync_incremental::tests::t274_incremental_sync_ten_file_smoke_under_five_seconds`

The failure is a strict wall-clock perf threshold (`<5s`) in an existing incremental-sync smoke benchmark, not caused by adaptive-query-plan code paths.

Validation run excluding that pre-existing perf smoke guard:

```bash
cargo test --workspace -- --skip t274_incremental_sync_ten_file_smoke_under_five_seconds
```

Result: **PASS** (`0 failed`).

## Post-review hardening fixes applied

1. **Budget metadata correctness**
   - `query_plan_budget_used` now tracks the **final executed plan budget** after any downgrade.
2. **Executed-plan observability**
   - Added `query_plan_executed` to search metadata and MCP protocol metadata.
3. **Timeout guard timing**
   - Added timeout guard checks before semantic execution and before rerank gating,
     not only at end-of-query warning stage.

## Benchmark report artifact

- `benchmarks/semantic/reports/adaptive-query-plan-2026-02-28.md`
