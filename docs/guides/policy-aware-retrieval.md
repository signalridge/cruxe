# Policy-Aware Retrieval Rollout Guide

This guide explains how to enable and operate policy-governed retrieval in Cruxe.

## What is enforced

Policy-aware retrieval applies deterministic controls before tool output is emitted:

1. **Path/type filtering** (deny + allow rules)
2. **Redaction** of common secret/PII patterns
3. **Metadata + audit counters** (`policy_mode`, blocked/redacted counters, warnings)

Supported per-request override parameter:

- `policy_mode` on `search_code` and `get_code_context` (only when `allow_request_override=true`)

## Policy modes

- `strict`: fail closed when policy configuration/override is invalid.
- `balanced`: enforce policy and fail open with explicit warnings when non-fatal issues occur.
- `off`: disable filtering/redaction.
- `audit_only`: record counters and warnings without mutating payloads.

## Recommended rollout sequence (audit-first)

### Phase 0 — Baseline (`off`)

Keep existing behavior while collecting operational baseline.

### Phase 1 — Audit (`audit_only`)

Use this first in production-like repos. It reports `policy_blocked_count` and
`policy_redacted_count` without changing response content.

### Phase 2 — Balanced (`balanced`)

Enable enforcement with fail-open safety. This is the default recommendation for
teams that want protection with operational resilience.

### Phase 3 — Strict (`strict`)

Enable for regulated repos once rules are tuned and false-positive rates are acceptable.

## Config templates

Starter templates are provided in:

- `configs/policy/strict-enterprise.toml`
- `configs/policy/balanced-product.toml`
- `configs/policy/audit-only-oss.toml`

Merge the selected section into `.cruxe/config.toml` (or pass via explicit config file).

## Detector baseline and provenance

Built-in default redaction categories are seeded from curated, high-signal secret families:

- PEM private key headers
- AWS access key ID prefixes
- GitHub token prefixes
- Slack token prefixes
- GCP service-account markers
- Email masking (configurable)
- Generic high-entropy token heuristic

Additional extension options:

- `search.policy.detect_secrets.plugins` (compatibility layer)
- `search.policy.detect_secrets.custom_patterns`
- `search.policy.redaction.custom_rules`
- `search.policy.opa.*` prototype hook

## Troubleshooting false positives / false negatives

### False positives (too much blocked/redacted)

Symptoms:

- Relevant code is missing from results
- `policy_blocked_count` unexpectedly high

Actions:

1. Switch to `audit_only` to inspect what would have been filtered.
2. Narrow `search.policy.path.deny` patterns.
3. Add `search.policy.path.allow` for approved high-value paths.
4. Disable/adjust aggressive custom redaction rules.

### False negatives (sensitive text still visible)

Symptoms:

- Secret-like strings appear unredacted
- `policy_redacted_count` remains near zero on known-sensitive fixtures

Actions:

1. Enable detect-secrets plugin mappings (`aws`, `github`, `slack`, `privatekey`).
2. Add `search.policy.detect_secrets.custom_patterns` for organization-specific formats.
3. Add targeted `search.policy.redaction.custom_rules`.
4. Increase entropy sensitivity (`high_entropy_min_length` / `high_entropy_threshold` tuning).

### Override behavior confusion

Symptoms:

- `policy_mode` request parameter is ignored/rejected

Actions:

1. Confirm `search.policy.allow_request_override=true`.
2. Ensure requested mode is listed in `allowed_override_modes`.
3. In `strict`, disallowed override is rejected deterministically.

### OPA hook errors

Symptoms:

- Warnings mention OPA evaluation failure

Actions:

1. Verify `opa` CLI availability.
2. Verify `search.policy.opa.policy_path` exists and is readable.
3. Validate query path (default `data.cruxe.allow`).
4. Use `balanced`/`audit_only` while stabilizing OPA policy execution.
