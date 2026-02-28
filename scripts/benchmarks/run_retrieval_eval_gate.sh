#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

REPORT_DIR="benchmarks/semantic/reports"
mkdir -p "$REPORT_DIR"

STAMP="$(date -u +"%Y-%m-%dT%H-%M-%SZ")"
JSON_OUT="$REPORT_DIR/policy-aware-retrieval-gate-$STAMP.json"
MD_OUT="$REPORT_DIR/policy-aware-retrieval-gate-$STAMP.md"

echo "[retrieval-eval-gate] Running policy mode evaluation example..."
cargo run -p cruxe-query --example policy_eval_gate > "$JSON_OUT"

python - "$JSON_OUT" "$MD_OUT" <<'PY'
import json
import sys
from pathlib import Path

json_path = Path(sys.argv[1])
md_path = Path(sys.argv[2])
rows = json.loads(json_path.read_text())

lines = []
lines.append("# Policy-Aware Retrieval Eval Gate Report")
lines.append("")
lines.append(f"- Source JSON: `{json_path}`")
lines.append("")
lines.append("| Mode | Emitted | Blocked | Redacted | Warnings |")
lines.append("|------|---------|---------|----------|----------|")
for row in rows:
    lines.append(
        f"| {row['mode']} | {row['emitted']} | {row['blocked']} | {row['redacted']} | {row['warnings']} |"
    )
lines.append("")
lines.append("## Interpretation")
lines.append("")
lines.append("- `off` is baseline pass-through.")
lines.append("- `audit_only` reports policy counters without mutating output.")
lines.append("- `balanced` enforces policy with fail-open warning behavior.")
lines.append("- `strict` enforces policy and fails closed on policy-load failures.")
md_path.write_text("\n".join(lines) + "\n")
PY

echo "[retrieval-eval-gate] JSON report: $JSON_OUT"
echo "[retrieval-eval-gate] Markdown report: $MD_OUT"
