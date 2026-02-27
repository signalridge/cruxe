#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: benchmarks/semantic/run_semantic_benchmarks.sh [--output <dir>]

Generate a deterministic semantic benchmark report key from:
1) fixtures.lock.json SHA-256
2) query-pack.v1.json SHA-256
3) current git revision

Output: report-<run_key>.json in target/semantic-benchmark-reports (default) or --output dir.
EOF
}

sha256_file() {
  local input="$1"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$input" | awk '{print $1}'
    return
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$input" | awk '{print $1}'
    return
  fi
  if command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$input" | awk '{print $NF}'
    return
  fi
  echo "missing sha256 tool (shasum/sha256sum/openssl)" >&2
  exit 1
}

sha256_text() {
  local input="$1"
  if command -v shasum >/dev/null 2>&1; then
    printf '%s' "$input" | shasum -a 256 | awk '{print $1}'
    return
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s' "$input" | sha256sum | awk '{print $1}'
    return
  fi
  if command -v openssl >/dev/null 2>&1; then
    printf '%s' "$input" | openssl dgst -sha256 | awk '{print $NF}'
    return
  fi
  echo "missing sha256 tool (shasum/sha256sum/openssl)" >&2
  exit 1
}

count_language_queries() {
  local query_pack="$1"
  local language="$2"
  grep -o "\"language\":\"${language}\"" "$query_pack" | wc -l | tr -d '[:space:]'
}

count_queries_total() {
  local query_pack="$1"
  grep -o '"id":"[^"]*"' "$query_pack" | wc -l | tr -d '[:space:]'
}

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
FIXTURES_FILE="${SCRIPT_DIR}/fixtures.lock.json"
QUERY_PACK_FILE="${SCRIPT_DIR}/query-pack.v1.json"
OUTPUT_DIR="${REPO_ROOT}/target/semantic-benchmark-reports"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      if [[ $# -lt 2 ]]; then
        echo "--output requires a directory argument" >&2
        exit 2
      fi
      OUTPUT_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [[ ! -f "${FIXTURES_FILE}" ]]; then
  echo "missing fixtures file: ${FIXTURES_FILE}" >&2
  exit 1
fi
if [[ ! -f "${QUERY_PACK_FILE}" ]]; then
  echo "missing query pack file: ${QUERY_PACK_FILE}" >&2
  exit 1
fi

mkdir -p "${OUTPUT_DIR}"

FIXTURES_SHA="$(sha256_file "${FIXTURES_FILE}")"
QUERY_PACK_SHA="$(sha256_file "${QUERY_PACK_FILE}")"
GIT_REVISION="$(git -C "${REPO_ROOT}" rev-parse HEAD 2>/dev/null || echo unknown)"
RUN_KEY="$(sha256_text "${FIXTURES_SHA}:${QUERY_PACK_SHA}:${GIT_REVISION}")"

REPORT_PATH="${OUTPUT_DIR}/report-${RUN_KEY}.json"
TMP_REPORT="${REPORT_PATH}.tmp.$$"

if [[ -f "${REPORT_PATH}" ]]; then
  printf '%s\n' "${REPORT_PATH}"
  exit 0
fi

QUERY_COUNT_TOTAL="$(count_queries_total "${QUERY_PACK_FILE}")"
RUST_COUNT="$(count_language_queries "${QUERY_PACK_FILE}" "rust")"
TYPESCRIPT_COUNT="$(count_language_queries "${QUERY_PACK_FILE}" "typescript")"
PYTHON_COUNT="$(count_language_queries "${QUERY_PACK_FILE}" "python")"
GO_COUNT="$(count_language_queries "${QUERY_PACK_FILE}" "go")"
GENERATED_AT_UTC="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

cat > "${TMP_REPORT}" <<EOF
{
  "version": "semantic-benchmark-report-v1",
  "generated_at_utc": "${GENERATED_AT_UTC}",
  "inputs": {
    "run_key": "${RUN_KEY}",
    "fixtures_sha256": "${FIXTURES_SHA}",
    "query_pack_sha256": "${QUERY_PACK_SHA}",
    "git_revision": "${GIT_REVISION}"
  },
  "summary": {
    "query_count_total": ${QUERY_COUNT_TOTAL},
    "query_count_by_language": {
      "rust": ${RUST_COUNT},
      "typescript": ${TYPESCRIPT_COUNT},
      "python": ${PYTHON_COUNT},
      "go": ${GO_COUNT}
    }
  },
  "metrics": {
    "latency_p95_ms": null,
    "mrr_hybrid": null,
    "mrr_lexical": null,
    "mrr_delta_percent": null,
    "rss_overhead_percent": null,
    "index_size_ratio": null
  },
  "notes": [
    "Run key is deterministic for fixtures/query-pack/git-revision inputs.",
    "Populate metrics fields from measured benchmark executions."
  ]
}
EOF

mv "${TMP_REPORT}" "${REPORT_PATH}"
printf '%s\n' "${REPORT_PATH}"
