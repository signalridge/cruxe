#!/usr/bin/env bash
set -euo pipefail

echo "Running CodeCompass MCP benchmark harness (ignored benchmark tests)..."

cargo test -p codecompass-mcp benchmark_t138_get_file_outline_p95_under_50ms -- --ignored --nocapture
cargo test -p codecompass-mcp benchmark_t457_first_query_p95_under_400ms -- --ignored --nocapture
cargo test -p codecompass-mcp benchmark_t457_health_endpoint_p95_under_50ms -- --ignored --nocapture
cargo test -p codecompass-mcp benchmark_t457_workspace_routing_overhead_p95_under_5ms -- --ignored --nocapture

echo "Benchmark harness run complete."
