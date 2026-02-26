#!/usr/bin/env bash
set -euo pipefail

echo "Running CodeCompass MCP benchmark harness (ignored benchmark tests)..."

cargo test -p codecompass-mcp benchmark_t138_get_file_outline_p95_under_50ms -- --ignored --nocapture
cargo test -p codecompass-mcp benchmark_t457_first_query_p95_under_400ms -- --ignored --nocapture
cargo test -p codecompass-mcp benchmark_t457_health_endpoint_p95_under_50ms -- --ignored --nocapture
cargo test -p codecompass-mcp benchmark_t457_workspace_routing_overhead_p95_under_5ms -- --ignored --nocapture
cargo test -p codecompass-cli benchmark_t359_call_edge_extraction_overhead_under_20_percent -- --ignored --nocapture
cargo test -p codecompass-query benchmark_t360_get_call_graph_p95_depth1_depth2_under_500ms -- --ignored --nocapture

echo "Benchmark harness run complete."
