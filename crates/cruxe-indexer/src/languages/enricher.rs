use cruxe_core::types::SymbolKind;

/// Language-specific enrichment for Tag → ExtractedSymbol conversion.
///
/// tree-sitter-tags provides symbol name, kind string, and location.
/// Each enricher fills the gaps: qualified names, visibility, parent scope,
/// and fine-grained kind mapping that the generic tags system cannot provide.
pub trait LanguageEnricher: Send + Sync {
    /// Language identifier (e.g., "rust", "go").
    fn language(&self) -> &'static str;

    /// Qualified name separator ("::" for Rust, "." for Python/Go/TS).
    fn separator(&self) -> &'static str;

    /// Map tag kind string to SymbolKind, with contextual refinement.
    ///
    /// `tag_kind` comes from `TagsConfiguration::syntax_type_name()` (e.g., "function", "class").
    /// `has_parent` indicates whether the symbol is nested inside another scope.
    /// `node` is the tree-sitter definition node for further inspection (e.g., distinguish
    /// struct vs enum when both are tagged as "class").
    fn map_kind(
        &self,
        tag_kind: &str,
        has_parent: bool,
        node: Option<tree_sitter::Node>,
        source: &str,
    ) -> Option<SymbolKind>;

    /// Extract visibility from the tree-sitter node at the tag's position.
    fn extract_visibility(&self, node: tree_sitter::Node, source: &str) -> Option<String>;

    /// Find the parent scope name by walking ancestors of `node`.
    fn find_parent_scope(&self, node: tree_sitter::Node, source: &str) -> Option<String>;
}

/// Helper: extract text from a tree-sitter node.
pub fn node_text<'a>(node: tree_sitter::Node, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

/// Strip generic arguments from a type-like name while preserving qualifiers.
///
/// Examples:
/// - `Foo<T>` -> `Foo`
/// - `pkg.Foo[T]` -> `pkg.Foo`
/// - `Result<Vec<T>, E>` -> `Result`
pub fn strip_generic_args(input: &str, open: char, close: char) -> String {
    let mut out = String::with_capacity(input.len());
    let mut depth = 0usize;

    for ch in input.chars() {
        if ch == open {
            depth += 1;
            continue;
        }
        if ch == close {
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth == 0 {
            out.push(ch);
        }
    }

    out.trim().to_string()
}
