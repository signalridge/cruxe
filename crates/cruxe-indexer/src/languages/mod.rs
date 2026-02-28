// Per-language modules (call sites + imports remain here).
pub mod go;
pub mod python;
pub mod rust;
pub mod typescript;

// tree-sitter-tags pipeline (replaces per-language symbol extraction).
pub mod generic_mapper;
pub mod tag_extract;
pub mod tag_registry;
pub(crate) mod text;

use cruxe_core::types::SymbolKind;

/// Extracted symbol from tree-sitter.
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub language: String,
    pub signature: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
    pub visibility: Option<String>,
    pub parent_name: Option<String>,
    pub body: Option<String>,
}

/// Extracted call-site from tree-sitter source traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedCallSite {
    pub callee_name: String,
    pub line: u32,
    pub confidence: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SymbolExtractionDiagnostics {
    pub had_tag_parse_error: bool,
}

/// Extract symbols using tree-sitter-tags + language enricher.
///
/// The pre-parsed `tree` is used for enrichment (parent walking, visibility
/// extraction, kind disambiguation) while `tree-sitter-tags` supplies canonical
/// definition tags.
pub fn extract_symbols(
    tree: &tree_sitter::Tree,
    source: &str,
    language: &str,
) -> Vec<ExtractedSymbol> {
    extract_symbols_with_diagnostics(tree, source, language).0
}

pub fn extract_symbols_with_diagnostics(
    tree: &tree_sitter::Tree,
    source: &str,
    language: &str,
) -> (Vec<ExtractedSymbol>, SymbolExtractionDiagnostics) {
    let (symbols, diagnostics) =
        tag_extract::extract_symbols_via_tags_with_diagnostics(tree, source, language);
    (
        symbols,
        SymbolExtractionDiagnostics {
            had_tag_parse_error: diagnostics.had_parse_error,
        },
    )
}

/// Extract call-sites from a parsed tree for a given language.
pub fn extract_call_sites(
    tree: &tree_sitter::Tree,
    source: &str,
    language: &str,
) -> Vec<ExtractedCallSite> {
    match language {
        "rust" => rust::extract_call_sites(tree, source),
        "typescript" => typescript::extract_call_sites(tree, source),
        "python" => python::extract_call_sites(tree, source),
        "go" => go::extract_call_sites(tree, source),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    #[test]
    fn go_extract_symbols_includes_const_and_var_definitions() {
        let source = r#"
package demo

const MaxRetries = 3
var retryCount = 0
"#;
        let tree = parse_file(source, "go").expect("parse go");
        let symbols = extract_symbols(&tree, source, "go");

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "MaxRetries" && s.kind == SymbolKind::Constant),
            "expected MaxRetries constant symbol"
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "retryCount" && s.kind == SymbolKind::Variable),
            "expected retryCount variable symbol"
        );
    }

    #[test]
    fn typescript_extract_symbols_includes_var_declarations() {
        let source = r#"
var legacyCount = 1;
"#;
        let tree = parse_file(source, "typescript").expect("parse typescript");
        let symbols = extract_symbols(&tree, source, "typescript");

        assert!(
            symbols
                .iter()
                .any(|s| s.name == "legacyCount" && s.kind == SymbolKind::Variable),
            "expected legacyCount variable symbol"
        );
    }
}
