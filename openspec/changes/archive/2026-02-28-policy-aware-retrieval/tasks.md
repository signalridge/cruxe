## 1. Policy model and configuration

- [x] 1.1 Define policy mode enum and configuration schema.
- [x] 1.2 Implement policy loader with strict/balanced/off semantics.
- [x] 1.3 Add config validation and startup diagnostics.
- [x] 1.4 Add tests for fail-closed vs fail-open behavior.

## 2. Filtering and redaction engine

- [x] 2.1 Implement path/type deny-allow filtering in retrieval pipeline.
- [x] 2.2 Implement redaction detectors for common secret/PII patterns.
- [x] 2.3 Apply redaction consistently to search snippets and context packs.
- [x] 2.4 Add tests for blocked, redacted, and pass-through scenarios.

## 3. Protocol and observability integration

- [x] 3.1 Add request controls for policy mode override (if allowed by config).
- [x] 3.2 Add response metadata fields (`policy_mode`, blocked/redacted counts).
- [x] 3.3 Add audit counters/logs for policy decisions.
- [x] 3.4 Preserve compatibility for clients that ignore new metadata.

## 4. Rollout support

- [x] 4.1 Add audit-only rollout mode and docs.
- [x] 4.2 Add example policy config templates for common repository types.
- [x] 4.3 Add troubleshooting guide for false positives/negatives.

## 5. Verification

- [x] 5.1 Run `cargo test --workspace`.
- [x] 5.2 Run `cargo clippy --workspace`.
- [x] 5.3 Run retrieval-eval-gate with policy modes to quantify impact.
- [x] 5.4 Attach OpenSpec evidence including blocked/redacted sample outputs.

## 6. Policy/rule ecosystem integration

- [x] 6.1 Seed default secret detectors from curated Gitleaks rule families.
- [x] 6.2 Add detect-secrets plugin compatibility layer for extensible redaction checks.
- [x] 6.3 Prototype optional OPA policy hook for auditable allow/deny decisions.
