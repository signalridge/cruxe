use cruxe_core::types::{QueryIntent, RefScope};

/// Query plan: which indices to search and with what weights.
#[derive(Debug, Clone)]
pub struct QueryPlan {
    pub intent: QueryIntent,
    pub ref_scope: RefScope,
    pub search_symbols: bool,
    pub search_snippets: bool,
    pub search_files: bool,
    pub symbol_weight: f32,
    pub snippet_weight: f32,
    pub file_weight: f32,
}

/// Resolve the effective ref for a query.
///
/// Priority: explicit ref > HEAD detection > "live" fallback.
pub fn resolve_ref(explicit_ref: Option<&str>, workspace: Option<&std::path::Path>) -> RefScope {
    // 1. Explicit ref parameter takes priority
    if let Some(r) = explicit_ref {
        return RefScope::explicit(r);
    }

    // 2. Try HEAD detection if workspace is a git repo
    if let Some(ws) = workspace
        && let Ok(branch) = cruxe_core::vcs::detect_head_branch(ws)
    {
        return RefScope::explicit(branch);
    }

    // 3. Fallback to "live"
    RefScope::live()
}

/// Build a query plan based on the classified intent.
pub fn build_plan(intent: QueryIntent) -> QueryPlan {
    build_plan_with_ref(intent, RefScope::live())
}

/// Build a query plan with a specific ref scope.
pub fn build_plan_with_ref(intent: QueryIntent, ref_scope: RefScope) -> QueryPlan {
    match intent {
        QueryIntent::Symbol => QueryPlan {
            intent,
            ref_scope,
            search_symbols: true,
            search_snippets: true,
            search_files: false,
            symbol_weight: 3.0,
            snippet_weight: 1.0,
            file_weight: 0.0,
        },
        QueryIntent::Path => QueryPlan {
            intent,
            ref_scope,
            search_symbols: false,
            search_snippets: false,
            search_files: true,
            symbol_weight: 0.0,
            snippet_weight: 0.0,
            file_weight: 3.0,
        },
        QueryIntent::Error => QueryPlan {
            intent,
            ref_scope,
            search_symbols: false,
            search_snippets: true,
            search_files: true,
            symbol_weight: 0.0,
            snippet_weight: 3.0,
            file_weight: 1.0,
        },
        QueryIntent::NaturalLanguage => QueryPlan {
            intent,
            ref_scope,
            search_symbols: true,
            search_snippets: true,
            search_files: true,
            symbol_weight: 2.0,
            snippet_weight: 2.0,
            file_weight: 1.0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::constants::REF_LIVE;

    #[test]
    fn test_resolve_ref_explicit_takes_priority() {
        let scope = resolve_ref(Some("feat/auth"), None);
        assert_eq!(scope.r#ref, "feat/auth");
        assert!(scope.is_explicit);
    }

    #[test]
    fn test_resolve_ref_falls_back_to_live() {
        // Non-git temp dir, no explicit ref -> falls back to "live"
        let dir = tempfile::tempdir().unwrap();
        let scope = resolve_ref(None, Some(dir.path()));
        assert_eq!(scope.r#ref, REF_LIVE);
        assert!(!scope.is_explicit);
    }

    #[test]
    fn test_resolve_ref_no_workspace_falls_back_to_live() {
        let scope = resolve_ref(None, None);
        assert_eq!(scope.r#ref, REF_LIVE);
        assert!(!scope.is_explicit);
    }

    #[test]
    fn test_build_plan_symbol_intent() {
        let plan = build_plan(QueryIntent::Symbol);
        assert!(plan.search_symbols);
        assert!(plan.search_snippets);
        assert!(!plan.search_files);
        assert_eq!(plan.ref_scope.r#ref, REF_LIVE);
    }

    #[test]
    fn test_build_plan_with_ref() {
        let scope = RefScope::explicit("main");
        let plan = build_plan_with_ref(QueryIntent::Path, scope);
        assert_eq!(plan.ref_scope.r#ref, "main");
        assert!(plan.ref_scope.is_explicit);
        assert!(plan.search_files);
        assert!(!plan.search_symbols);
    }

    /// T072: Single-version mode (no Git) defaults to ref="live".
    #[test]
    fn t072_single_version_mode_defaults_to_live() {
        // Simulate a non-VCS project: no workspace (None) and no explicit ref
        let scope = resolve_ref(None, None);
        assert_eq!(scope.r#ref, "live");
        assert!(!scope.is_explicit);

        // Also test with a non-git workspace dir
        let dir = tempfile::tempdir().unwrap();
        let scope2 = resolve_ref(None, Some(dir.path()));
        assert_eq!(scope2.r#ref, "live");
        assert!(!scope2.is_explicit);

        // build_plan defaults to "live"
        let plan = build_plan(QueryIntent::Symbol);
        assert_eq!(plan.ref_scope.r#ref, "live");
        assert!(!plan.ref_scope.is_explicit);
    }
}
