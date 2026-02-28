## Context

Cruxe is local-first and often used on sensitive repositories. Existing retrieval paths optimize relevance but do not provide first-class policy enforcement and payload redaction. This limits safe adoption in regulated environments.

## Goals / Non-Goals

Phase-1 scope boundary:
- deterministic path policy filtering,
- deterministic redaction primitives,
- policy observability metadata.

Out-of-scope in phase 1:
- full external policy DSL integration,
- advanced ML-based DLP classification.

**Goals:**
1. Enforce deterministic policy filtering before response emission.
2. Provide practical redaction for obvious secrets/PII in snippets.
3. Keep policy behavior explicit in metadata/audit counters.
4. Ensure strict mode is fail-closed for unknown policy state.

**Non-Goals:**
1. Full DLP/classification platform.
2. Perfect secret detection in every language format.
3. Cloud policy management integration.

## Decisions

### D1. Policy engine order
Order:
1) hard deny filters,
2) allowlist checks,
3) redaction pass,
4) ranking/packing emission.

**Why:** prevents denied content from influencing downstream response content.

### D2. Three policy modes
- `strict`: fail closed on policy load errors; stronger defaults.
- `balanced`: fail open with explicit warnings.
- `off`: policy pass-through (for local experimentation).

**Why:** matches different trust/velocity needs.

### D3. Redaction primitives
Use deterministic regex/heuristic detectors for common tokens (API keys, private keys, emails, high-entropy literals), with pluggable rules.

**Why:** practical baseline with bounded complexity.

### D4. Metadata and auditability
Expose metadata:
- `policy_mode`
- `policy_blocked_count`
- `policy_redacted_count`
- `policy_warnings`

**Why:** users need to understand why results differ.

## Risks / Trade-offs

- **[Risk] False positives reduce retrieval usefulness** → Mitigation: allow scoped policy overrides and rule tuning.
- **[Risk] False negatives leave residual risk** → Mitigation: document detector limits and encourage layered controls.
- **[Risk] Strict mode can block workflows unexpectedly** → Mitigation: dry-run mode and rollout with audit-only phase.

## Migration Plan

1. Implement audit-only mode (report but do not block/redact).
2. Enable `balanced` mode by config for pilot repositories.
3. Enable `strict` mode where required by governance.
4. Publish operational guidance and troubleshooting docs.

Rollback: revert to `off` while preserving audit instrumentation for investigation.

## Resolved Defaults

1. Policy configuration is workspace-scoped in phase 1 with optional future ref-specific override.
2. A single policy profile applies to both `search_code` and `build_context_pack` in phase 1.

## External References (2026-02-28 Investigation)

Investigated related open-source projects and extracted directly applicable design constraints:

- **gitleaks/gitleaks** (Go, stars=25147)
  - Upstream focus: Find secrets with Gitleaks 🔑
  - Local clone: `<ghq>/github.com/gitleaks/gitleaks`
  - Applied insight: High-signal secret pattern corpus and scanning rule design.
  - Source: https://github.com/gitleaks/gitleaks
- **yelp/detect-secrets** (Python, stars=4430)
  - Upstream focus: An enterprise friendly way of detecting and preventing secrets in code.
  - Local clone: `<ghq>/github.com/Yelp/detect-secrets`
  - Applied insight: Plugin-based secret detection architecture and baseline workflows.
  - Source: https://github.com/Yelp/detect-secrets
- **open-policy-agent/opa** (Go, stars=11278)
  - Upstream focus: Open Policy Agent (OPA) is an open source, general-purpose policy engine.
  - Local clone: `<ghq>/github.com/open-policy-agent/opa`
  - Applied insight: Policy DSL/runtime for auditable allow/deny decisions.
  - Source: https://github.com/open-policy-agent/opa
