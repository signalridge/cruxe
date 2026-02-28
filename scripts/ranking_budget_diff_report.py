#!/usr/bin/env python3
"""Generate markdown diff report for ranking budget evaluations.

Usage:
  python scripts/ranking_budget_diff_report.py \
    --before target/ranking-budget/pre.json \
    --after target/ranking-budget/post.json \
    --out target/ranking-budget/diff.md
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def pct(value: float) -> str:
    return f"{value * 100:.1f}%"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--before", required=True, type=Path)
    parser.add_argument("--after", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()

    before = load(args.before)
    after = load(args.after)

    metrics = [
        ("top1_hit_rate", "Top-1 hit rate"),
        ("mrr", "MRR"),
    ]

    lines = [
        "# Ranking Budget Contract Diff Report",
        "",
        f"- before profile: `{before.get('profile', 'unknown')}`",
        f"- after profile: `{after.get('profile', 'unknown')}`",
        f"- before file: `{args.before}`",
        f"- after file: `{args.after}`",
        "",
        "| Metric | Before | After | Delta |",
        "| --- | ---: | ---: | ---: |",
    ]
    for key, label in metrics:
        b = float(before.get(key, 0.0))
        a = float(after.get(key, 0.0))
        delta = a - b
        lines.append(f"| {label} | {pct(b)} | {pct(a)} | {pct(delta)} |")

    before_cases = {case["id"]: case for case in before.get("cases", [])}
    after_cases = {case["id"]: case for case in after.get("cases", [])}
    case_ids = sorted(set(before_cases) | set(after_cases))

    lines.extend(
        [
            "",
            "## Case-level deltas",
            "",
            "| Case | Before top1 | After top1 | Before RR | After RR |",
            "| --- | --- | --- | ---: | ---: |",
        ]
    )
    for case_id in case_ids:
        b = before_cases.get(case_id, {})
        a = after_cases.get(case_id, {})
        lines.append(
            "| {id} | `{b_top}` | `{a_top}` | {b_rr:.3f} | {a_rr:.3f} |".format(
                id=case_id,
                b_top=b.get("observed_top_result_id"),
                a_top=a.get("observed_top_result_id"),
                b_rr=float(b.get("reciprocal_rank", 0.0)),
                a_rr=float(a.get("reciprocal_rank", 0.0)),
            )
        )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote diff report: {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
