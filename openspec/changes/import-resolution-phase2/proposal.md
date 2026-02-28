## Why

Current import resolution in Cruxe still carries language-specific logic branches and hardcoded heuristics in `import_extract.rs`. This creates long-term maintenance and quality pressure.

For Cruxe's direction, we need:

- universal baseline resolution,
- low per-language maintenance,
- predictable indexing behavior with minimal complexity.

After evaluation, external compiler-grade adapter integration is removed from this phase because it adds complexity without sufficient near-term ROI for this framework.

## What Changes

We implement a **fast universal resolver architecture**:

1. **Resolver Provider Interface (universal core)**
   - Normalized provider contract with tri-state outcomes: `InternalResolved`, `ExternalReference`, `Unresolved`.

2. **Generic Heuristic Resolver (default, always on)**
   - Path normalization + relative resolution + manifest checks.
   - Language-agnostic candidate generation.

3. **No external evidence adapters in this change**
   - External adapter integration is explicitly out of scope.
   - Keep implementation focused on deterministic baseline behavior.

4. **Observability + revisit gate**
   - Emit resolution metrics and unresolved ratio.
   - Revisit external adapters only if metrics prove baseline is insufficient.

## Capabilities

### Modified Capabilities
- `import-path-resolution`: upgraded to provider-chain architecture with universal generic baseline resolver.
- `symbol-contract-v2`: unresolved import semantics remain stable (`to_symbol_id = NULL`) with provider diagnostics.

## Impact

- Affected crate: `cruxe-indexer`.
- API impact: none.
- Performance impact: no additional heavy integration in indexing/runtime path.
- Maintenance impact: avoids expanding per-language compiler-grade resolver logic in core.
