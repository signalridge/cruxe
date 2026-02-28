# Evidence — policy-aware-retrieval

## Verification Commands

Executed in worktree:

```bash
cargo test --workspace
cargo clippy --workspace
scripts/benchmarks/run_retrieval_eval_gate.sh
```

## Command Results

### 1) `cargo test --workspace`

- Status: ✅ PASS
- Summary highlights:
  - `cruxe-cli` integration tests: 21 passed, 7 ignored
  - `cruxe-core`: 58 passed
  - `cruxe-indexer`: 84 passed
  - `cruxe-mcp`: 126 passed, 4 ignored
  - `cruxe-query`: 117 passed, 1 ignored
  - `cruxe-state`: 135 passed
  - `cruxe-vcs`: 11 passed

### 2) `cargo clippy --workspace`

- Status: ✅ PASS (no warnings after final cleanup)

### 3) Retrieval eval gate with policy modes

- Script: `scripts/benchmarks/run_retrieval_eval_gate.sh`
- Local JSON report (timestamped output from script run):
  `benchmarks/semantic/reports/policy-aware-retrieval-gate-2026-02-28T16-20-58Z.json`

Mode comparison:

| Mode | Emitted | Blocked | Redacted | Warnings |
|------|---------|---------|----------|----------|
| off | 3 | 0 | 0 | 0 |
| audit_only | 3 | 1 | 3 | 0 |
| balanced | 2 | 1 | 2 | 0 |
| strict | 2 | 1 | 2 | 0 |

## Blocked/Redacted Sample Outputs

Extracted from eval gate JSON (`sample_outputs`):

### Off (pass-through)

```json
{
  "path": "src/secrets/keys.rs",
  "snippet": "const API_KEY: &str = \"ghp_abcdefghijklmnopqrstuvwxyz\";"
}
```

### Balanced / Strict (enforced redaction + blocked path)

```json
{
  "path": "src/notify.rs",
  "snippet": "send_email(\"[REDACTED:email]\", \"[REDACTED:aws_access_key]\")"
}
```

Blocked sample (not emitted in balanced/strict): `src/secrets/keys.rs`
