pub mod go;
pub mod python;
pub mod rust;
pub mod typescript;

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

/// Extract symbols from a parsed tree for a given language.
pub fn extract_symbols(
    tree: &tree_sitter::Tree,
    source: &str,
    language: &str,
) -> Vec<ExtractedSymbol> {
    match language {
        "rust" => rust::extract(tree, source),
        "typescript" => typescript::extract(tree, source),
        "python" => python::extract(tree, source),
        "go" => go::extract(tree, source),
        _ => Vec::new(),
    }
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
