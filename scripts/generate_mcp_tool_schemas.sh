#!/usr/bin/env bash
set -euo pipefail

WORKSPACE_PATH="${1:-.}"
OUTPUT_PATH="${2:-configs/mcp/tool-schemas.json}"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 1
fi

request='{"jsonrpc":"2.0","id":"schema-export","method":"tools/list","params":{}}'
stderr_log="$(mktemp -t cruxe-mcp-stderr.XXXXXX)"

response="$({
  printf '%s\n' "$request" |
    RUST_LOG=error cargo run -q -p cruxe -- serve-mcp --workspace "$WORKSPACE_PATH" --no-prewarm 2>"$stderr_log" |
    grep -m 1 '^{"jsonrpc":"2.0"' || true
})"

if [ -z "$response" ]; then
  echo "Failed to capture tools/list response" >&2
  echo "--- stderr ---" >&2
  cat "$stderr_log" >&2
  rm -f "$stderr_log"
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT_PATH")"

version="$(cargo metadata --no-deps --format-version=1 | jq -r '.packages[] | select(.name == "cruxe") | .version')"
generated_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

echo "$response" |
  jq --arg generated_at "$generated_at" --arg version "$version" '
    .result.tools as $tools
    | {
        generated_at: $generated_at,
        generator: "scripts/generate_mcp_tool_schemas.sh",
        source: "tools/list",
        binary: "cruxe",
        binary_version: $version,
        tool_count: ($tools | length),
        tools: $tools
      }
  ' >"$OUTPUT_PATH"

rm -f "$stderr_log"
echo "Wrote $OUTPUT_PATH"
