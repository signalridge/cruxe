## 1. Retrieval gate enforcement in CI

- [x] 1.1 Remove `--dry-run` from retrieval gate workflow step.
- [x] 1.2 Keep retrieval eval report artifact upload intact for diagnostics.

## 2. Query crate post-merge hardening

- [x] 2.1 Mark `semantic_fanout_limits` helper as test-only to remove dead-code warnings.
- [x] 2.2 Replace ranking/explain `f64::EPSILON` comparisons with explicit tolerance constants where semantic tolerance is intended.

## 3. Archived spec metadata quality

- [x] 3.1 Replace placeholder Purpose text in the six newly archived capability specs with concrete capability purpose statements.

## 4. Verification and governance

- [x] 4.1 Run formatting and compile checks for touched crates/workspace scope.
- [x] 4.2 Run targeted retrieval/ranking/context-pack tests.
- [x] 4.3 Validate OpenSpec artifacts for `post-merge-hardening` and strict spec checks.

## 5. Deep-review risk fixes (correctness/security/perf)

- [x] 5.1 Harden ranking budget scoring against non-finite inputs and ensure NaN scores sort deterministically.
- [x] 5.2 Harden policy OPA execution path validation and stdin lifecycle handling.
- [x] 5.3 Tighten policy redaction coverage (PEM full block) and symbol-kind allowlist behavior for missing kind.
- [x] 5.4 Improve retrieval eval target matching semantics and BEIR/TREC qrels parser compatibility.
- [x] 5.5 Add context-pack source-content caching and use ref-safe git object reads to reduce subprocess churn.
- [x] 5.6 Normalize adaptive-plan semantic-unavailable fallback to lexical-fast for all intents.

## 6. Spec alignment follow-ups (review gap closure)

- [x] 6.1 Keep `high_entropy` visible in default redaction category diagnostics and document built-in provenance in default config comments.
- [x] 6.2 Preserve lexical precedence during confidence-structural reordering even when ranking explain payload is disabled.
- [x] 6.3 Accept `section_caps.key_usages`/`section_caps.dependencies` aliases and document canonical mapping in MCP tool schema.
- [x] 6.4 Ensure `SuiteBaseline::from_metrics` emits non-zero latency distribution defaults and expose explicit latency constructor.
