# policy-aware-retrieval Specification

## Purpose
Define deterministic retrieval governance modes, policy filtering, and redaction controls so emitted search/context payloads obey safety constraints with auditable outcomes.

## Requirements
### Requirement: Retrieval MUST enforce policy profiles deterministically
Retrieval execution MUST support deterministic policy profiles: `strict`, `balanced`, `off`.

Profile semantics:
- `strict`: fail closed on policy load/validation errors,
- `balanced`: fail open with warnings,
- `off`: policy bypass.

#### Scenario: Strict mode fails closed on invalid policy
- **WHEN** policy mode is `strict` and policy configuration cannot be loaded
- **THEN** retrieval MUST reject emission of ungoverned sensitive content

#### Scenario: Balanced mode proceeds with warning
- **WHEN** policy mode is `balanced` and a non-fatal policy issue occurs
- **THEN** retrieval MUST continue and emit policy warning metadata

### Requirement: Policy filtering MUST run before final emission
Policy filtering (deny/allow decisions) MUST execute before final result emission for search and context-pack outputs.

#### Scenario: Denied path content is excluded
- **WHEN** a candidate originates from a denied path policy rule
- **THEN** candidate MUST NOT be emitted in final output

### Requirement: Redaction MUST be applied to sensitive snippet content
The system MUST apply deterministic redaction rules for configured secret/PII patterns before payload emission.

#### Scenario: Sensitive token is redacted in emitted snippet
- **WHEN** a snippet contains a configured secret pattern
- **THEN** emitted payload MUST redact the sensitive span
- **AND** redaction counters MUST increase

### Requirement: Default redaction rule set MUST be explicit
The phase-1 redaction baseline MUST define an explicit built-in rule set and deterministic placeholders.

Minimum baseline categories:
- PEM private key headers,
- high-confidence API token prefixes (provider-specific known prefixes),
- generic high-entropy token heuristic with bounded length thresholds,
- email address masking (configurable).

Concrete regex patterns and detection thresholds for each category are deferred to implementation.
The implementation MUST seed its default rules from established open-source corpora (gitleaks rule families and/or detect-secrets plugin patterns) rather than inventing patterns from scratch.

#### Scenario: Built-in redaction categories are auditable
- **WHEN** policy engine starts with default configuration
- **THEN** runtime MUST expose the active built-in redaction categories in diagnostics
- **AND** each category MUST list its active pattern count
- **AND** emitted redaction counters MUST be attributable to category names

#### Scenario: Default rules derive from established corpora
- **WHEN** no custom redaction rules are configured
- **THEN** the built-in rule set MUST cover at minimum: PEM headers, AWS/GCP/GitHub/Slack token prefixes, and generic high-entropy strings
- **AND** rule provenance (source corpus) MUST be documented in configuration defaults

