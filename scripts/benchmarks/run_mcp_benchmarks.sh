#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

echo "Running Cruxe benchmark harness (transport + semantic benchmark lanes)..."

cargo test -p cruxe-mcp benchmark_t138_get_file_outline_p95_under_50ms -- --ignored --nocapture
cargo test -p cruxe-mcp benchmark_t457_first_query_p95_under_400ms -- --ignored --nocapture
cargo test -p cruxe-mcp benchmark_t457_health_endpoint_p95_under_50ms -- --ignored --nocapture
cargo test -p cruxe-mcp benchmark_t457_workspace_routing_overhead_p95_under_5ms -- --ignored --nocapture
cargo test -p cruxe benchmark_t359_call_edge_extraction_overhead_under_20_percent -- --ignored --nocapture
cargo test -p cruxe-query benchmark_t360_get_call_graph_p95_depth1_depth2_under_500ms -- --ignored --nocapture
# Run semantic phase-8 benchmark tests in a single invocation so they share the
# same process-level OnceLock cache/report generation and avoid repeated setup.
cargo test -p cruxe-query --test semantic_phase8_benchmarks -- --ignored --nocapture

echo "Generating deterministic semantic benchmark report key..."
benchmarks/semantic/run_semantic_benchmarks.sh

echo "Benchmark harness run complete."
