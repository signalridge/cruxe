## 1. Plan model and selector

- [ ] 1.1 Define canonical plan enum and plan config structure.
- [ ] 1.2 Implement rule-first selector from intent/confidence/runtime state.
- [ ] 1.3 Add deterministic tests for selector precedence and tie-breaks.
- [ ] 1.4 Add explicit override handling (`plan=...`) with validation.

## 2. Budgeted execution and downgrade path

- [ ] 2.1 Define per-plan fanout and latency budget knobs.
- [ ] 2.2 Implement one-way downgrade controller with reason codes.
- [ ] 2.3 Ensure downgrade never hard-fails query responses.
- [ ] 2.4 Add integration tests for deep→standard→fast fallback behavior.

## 3. Metadata and protocol wiring

- [ ] 3.1 Add metadata fields for selected/downgraded plan details.
- [ ] 3.2 Thread plan metadata through MCP response paths.
- [ ] 3.3 Preserve backward compatibility for clients ignoring new fields.
- [ ] 3.4 Add protocol tests for metadata presence/absence.

## 4. Config and observability

- [ ] 4.1 Add config schema for plan thresholds and budgets.
- [ ] 4.2 Add startup normalization/lint for invalid plan configs.
- [ ] 4.3 Add counters by selected plan and downgrade reason.

## 5. Verification

- [ ] 5.1 Run `cargo test --workspace`.
- [ ] 5.2 Run `cargo clippy --workspace`.
- [ ] 5.3 Run retrieval-eval-gate with adaptive planning enabled and compare baseline.
- [ ] 5.4 Update OpenSpec evidence with latency/quality deltas.

## 6. Router-policy benchmark alignment

- [ ] 6.1 Add Haystack-style router fixtures for intent-based pipeline selection.
- [ ] 6.2 Add LlamaIndex-style ambiguous query fixtures to validate downgrade behavior.
- [ ] 6.3 Add benchmark assertions for plan-specific p95 budgets and downgrade rates.
