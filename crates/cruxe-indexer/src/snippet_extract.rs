use crate::languages::ExtractedSymbol;
use cruxe_core::types::SnippetRecord;

/// Build SnippetRecords from extracted symbols.
/// Each function/method/class body becomes a snippet.
pub fn build_snippet_records(
    extracted: &[ExtractedSymbol],
    repo: &str,
    r#ref: &str,
    path: &str,
    commit: Option<&str>,
) -> Vec<SnippetRecord> {
    extracted
        .iter()
        .filter_map(|sym| {
            let body = sym.body.as_ref()?;
            if body.trim().is_empty() {
                return None;
            }

            let chunk_type = match sym.kind {
                cruxe_core::types::SymbolKind::Function => "function_body",
                cruxe_core::types::SymbolKind::Method => "method_body",
                cruxe_core::types::SymbolKind::Class => "class_body",
                cruxe_core::types::SymbolKind::Struct => "struct_body",
                cruxe_core::types::SymbolKind::Trait => "trait_body",
                cruxe_core::types::SymbolKind::Interface => "interface_body",
                cruxe_core::types::SymbolKind::Module => "module_body",
                _ => return None, // Skip constants, variables, etc.
            };

            Some(SnippetRecord {
                repo: repo.to_string(),
                r#ref: r#ref.to_string(),
                commit: commit.map(String::from),
                path: path.to_string(),
                language: sym.language.clone(),
                chunk_type: chunk_type.to_string(),
                imports: None,
                line_start: sym.line_start,
                line_end: sym.line_end,
                content: body.clone(),
            })
        })
        .collect()
}
