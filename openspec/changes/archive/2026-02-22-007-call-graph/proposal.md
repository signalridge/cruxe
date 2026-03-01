## Why

AI coding agents need to understand call relationships between functions to reason about code changes and their impacts. Without call edge extraction and graph traversal, agents cannot answer questions like "what would break if I change this function?" or "what does this function depend on?" Additionally, comparing symbol evolution across commits and providing follow-up query suggestions when initial searches fail are critical for effective agent-assisted code review and search strategy refinement.

## What Changes

1. Extract call edges from source files using tree-sitter during indexing for Rust, TypeScript, Python, and Go, storing caller-callee relationships in the `symbol_edges` table with confidence levels.
2. Add a `get_call_graph` MCP tool that returns callers and/or callees for a given symbol with configurable depth (1-5) and limit, scoped by ref.
3. Add a `compare_symbol_between_commits` MCP tool that shows the diff of a symbol's signature, body, and line range between two refs.
4. Add a `suggest_followup_queries` MCP tool that analyzes previous query results and suggests next tool calls when confidence is low.

## Capabilities

### New Capabilities

- **Call Edge Extraction** [US1]: Tree-sitter-based call site matching during indexing, producing `edge_type = 'calls'` entries in `symbol_edges` with `static` or `heuristic` confidence. Cross-file resolution by qualified name matching. Unresolvable targets stored with `to_symbol_id = NULL`.
- **Call Graph Query** [US2]: `get_call_graph` MCP tool with BFS traversal, configurable direction (callers/callees/both), depth (1-5, capped), limit, and ref scoping. Cycle detection for recursive calls.
- **Symbol Comparison** [US3]: `compare_symbol_between_commits` MCP tool showing signature diff, body diff summary, and line range changes between two refs. Handles added, deleted, and unchanged symbols.
- **Follow-up Suggestions** [US4]: `suggest_followup_queries` MCP tool providing actionable tool call suggestions with parameters when previous query results have low confidence.

### Modified Capabilities

- **002-agent-protocol**: All three new tool responses include Protocol v1 metadata, stable symbol handles, and canonical error envelopes.

### Functional Requirements

- **FR-601**: System MUST extract call edges from source files using tree-sitter during indexing, matching function/method call sites for Rust, TypeScript, Python, and Go.
- **FR-602**: System MUST store call edges in `symbol_edges` table with `edge_type = 'calls'`, `from_symbol_id`, `to_symbol_id`, `confidence` (`static` or `heuristic`), and `source_location` (file path + line number of the call site).
- **FR-603**: System MUST resolve cross-file call targets best-effort by matching the callee name against the qualified names in the symbol index for the same ref.
- **FR-604**: System MUST record unresolvable call targets with `to_symbol_id = NULL` and `to_name` set to the best available name from the call site AST node.
- **FR-605**: System MUST provide a `get_call_graph` MCP tool that returns callers and/or callees for a given symbol, scoped by ref, with configurable depth (1-5) and limit.
- **FR-606**: System MUST provide a `compare_symbol_between_commits` MCP tool that shows the diff of a symbol's signature, body, and line range between two refs.
- **FR-607**: System MUST provide a `suggest_followup_queries` MCP tool that analyzes previous query results and suggests next tool calls when confidence is low.
- **FR-608**: System MUST cap call graph traversal depth at 5 and include a warning in metadata when the requested depth exceeds this limit.
- **FR-609**: System MUST include Protocol v1 metadata in all new tool responses.
- **FR-610**: System MUST handle recursive calls (self-edges) correctly in graph traversal without infinite loops.

### Key Entities

- **CallEdge**: A directed relationship from a caller symbol to a callee symbol, with confidence level, source location (call site file + line), and edge type.
- **CallGraph**: A directed graph of symbols connected by call edges, with traversal bounded by depth and result count limits.
- **SymbolComparison**: A diff between two versions of the same symbol across different refs, including signature, body, and line range changes.
- **FollowupSuggestion**: A recommended tool call with parameters and rationale, generated from analysis of previous query results.

## Impact

### Success Criteria

- **SC-601**: Call edge extraction achieves >= 80% precision for direct (static) calls on fixture repositories with known call graphs.
- **SC-602**: `get_call_graph` returns results within 500ms p95 for depth <= 2 on a repository with 10,000 symbols.
- **SC-603**: `compare_symbol_between_commits` returns accurate diff summaries for at least 90% of symbol changes in a fixture PR.
- **SC-604**: `suggest_followup_queries` provides actionable suggestions that, when followed, improve result quality in >= 70% of low-confidence scenarios.

### Edge Cases

- Recursive function calls itself: a self-referencing edge is created with `from_symbol_id = to_symbol_id`.
- Call site uses a function pointer or closure: recorded with `confidence = 'heuristic'` and `to_name` set to the variable name. Resolution to the actual target is not attempted.
- `get_call_graph` called with `depth > 5`: depth is capped at 5 to prevent runaway graph traversal. A warning is included in metadata.
- `compare_symbol_between_commits` references a ref that is not indexed: an error is returned: `ref_not_indexed` with the unindexed ref name.
- Call edge extraction encounters a syntax error in the source file: the file is skipped for call edge extraction (but still indexed for symbols), and a warning is logged.

### Affected Crates

- `cruxe-indexer` (new `call_extract.rs` module, language-specific call site patterns)
- `cruxe-state` (new call edge CRUD in `edges.rs`)
- `cruxe-query` (new `call_graph.rs`, `symbol_compare.rs`, `followup.rs`)
- `cruxe-mcp` (three new MCP tool handlers)
- `cruxe-core` (new `CallEdge` type)

### API Impact

- Additive only: three new MCP tools (`get_call_graph`, `compare_symbol_between_commits`, `suggest_followup_queries`).

### Performance Impact

- Call edge extraction adds < 20% overhead to indexing time.
- Call graph query p95 < 500ms for depth <= 2 on 10,000-symbol index.
- Scale target: up to 50,000 symbols, up to 200,000 call edges per repo.

### Readiness Baseline Note (2026-02-25)

`symbol_edges` forward/reverse typed lookup indexes and query-plan regression tests are now in place to support call-graph traversal latency targets.
