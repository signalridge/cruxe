## 1. Policy model and configuration

- [ ] 1.1 Define policy mode enum and configuration schema.
- [ ] 1.2 Implement policy loader with strict/balanced/off semantics.
- [ ] 1.3 Add config validation and startup diagnostics.
- [ ] 1.4 Add tests for fail-closed vs fail-open behavior.

## 2. Filtering and redaction engine

- [ ] 2.1 Implement path/type deny-allow filtering in retrieval pipeline.
- [ ] 2.2 Implement redaction detectors for common secret/PII patterns.
- [ ] 2.3 Apply redaction consistently to search snippets and context packs.
- [ ] 2.4 Add tests for blocked, redacted, and pass-through scenarios.

## 3. Protocol and observability integration

- [ ] 3.1 Add request controls for policy mode override (if allowed by config).
- [ ] 3.2 Add response metadata fields (`policy_mode`, blocked/redacted counts).
- [ ] 3.3 Add audit counters/logs for policy decisions.
- [ ] 3.4 Preserve compatibility for clients that ignore new metadata.

## 4. Rollout support

- [ ] 4.1 Add audit-only rollout mode and docs.
- [ ] 4.2 Add example policy config templates for common repository types.
- [ ] 4.3 Add troubleshooting guide for false positives/negatives.

## 5. Verification

- [ ] 5.1 Run `cargo test --workspace`.
- [ ] 5.2 Run `cargo clippy --workspace`.
- [ ] 5.3 Run retrieval-eval-gate with policy modes to quantify impact.
- [ ] 5.4 Attach OpenSpec evidence including blocked/redacted sample outputs.

## 6. Policy/rule ecosystem integration

- [ ] 6.1 Seed default secret detectors from curated Gitleaks rule families.
- [ ] 6.2 Add detect-secrets plugin compatibility layer for extensible redaction checks.
- [ ] 6.3 Prototype optional OPA policy hook for auditable allow/deny decisions.
