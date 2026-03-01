## Context

Import resolution quality improves graph continuity, but Cruxe also needs responsive and low-complexity indexing behavior. For this phase, the design goal is a strong universal baseline without introducing external adapter complexity.

## Goals / Non-Goals

**Goals**
1. Replace branch-heavy import logic with provider-based architecture.
2. Keep generic resolver as universal always-on baseline.
3. Preserve fail-soft unresolved semantics and strong observability.
4. Keep implementation simple and maintainable.

**Non-Goals**
1. Mandatory external dependency for successful indexing.
2. Compiler-parity per-language resolver implementation in Cruxe core.
3. External heavy adapter integration in this phase.

## Decisions

### D1. Provider-based resolver contract

Introduce normalized outcomes:

```rust
enum ResolveOutcome {
    InternalResolved { to_symbol_id: String, to_name: String },
    ExternalReference { to_name: String },
    Unresolved { to_name: String },
}

trait ImportResolverProvider {
    fn name(&self) -> &'static str;
    fn resolve(&self, req: &ResolveRequest, ctx: &ResolveContext) -> Option<ResolveOutcome>;
}
```

Deterministic chain, first `Some(outcome)` wins.

### D2. Generic Heuristic Resolver baseline

Always-on baseline provider:

1. normalize import/source paths,
2. resolve relative references,
3. generate language-agnostic candidates,
4. validate against manifest + symbol relations.

This lane is the sole execution path in this change.

### D3. External adapters explicitly deferred

External heavy adapters are removed from this change scope.

Revisit condition (future change only):
- unresolved ratio remains high after baseline rollout,
- and measured value justifies added complexity.

### D4. Observability and semantics

- unresolved/external keep `to_symbol_id = NULL`.
- emit per-provider counters and unresolved ratios.
- emit per-run `import_resolution_rate` for tuning.

## Risks / Trade-offs

- **Risk: baseline may under-resolve some alias-heavy repos.**
  - Mitigation: metric-driven future follow-up, not immediate complexity increase.

- **Trade-off: no external-evidence precision boost in this phase.**
  - Accepted to keep architecture lean and implementation velocity high.
