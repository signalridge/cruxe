use crate::languages::ExtractedSymbol;
use cruxe_core::types::{SymbolRecord, compute_symbol_id, compute_symbol_stable_id};

/// Build SymbolRecords from extracted symbols.
pub fn build_symbol_records(
    extracted: &[ExtractedSymbol],
    repo: &str,
    r#ref: &str,
    path: &str,
    commit: Option<&str>,
) -> Vec<SymbolRecord> {
    extracted
        .iter()
        .map(|sym| {
            let symbol_id =
                compute_symbol_id(repo, r#ref, path, &sym.kind, sym.line_start, &sym.name);
            let symbol_stable_id = compute_symbol_stable_id(
                &sym.language,
                &sym.kind,
                &sym.qualified_name,
                sym.signature.as_deref(),
            );

            let parent_symbol_id = sym.parent_name.as_ref().and_then(|parent_name| {
                // Find the parent in the extracted symbols to get its ID
                extracted
                    .iter()
                    .find(|s| s.name == *parent_name && s.line_start < sym.line_start)
                    .map(|parent| {
                        compute_symbol_id(
                            repo,
                            r#ref,
                            path,
                            &parent.kind,
                            parent.line_start,
                            &parent.name,
                        )
                    })
            });

            SymbolRecord {
                repo: repo.to_string(),
                r#ref: r#ref.to_string(),
                commit: commit.map(String::from),
                path: path.to_string(),
                language: sym.language.clone(),
                symbol_id,
                symbol_stable_id,
                name: sym.name.clone(),
                qualified_name: sym.qualified_name.clone(),
                kind: sym.kind,
                signature: sym.signature.clone(),
                line_start: sym.line_start,
                line_end: sym.line_end,
                parent_symbol_id,
                visibility: sym.visibility.clone(),
                content: sym.body.clone(),
            }
        })
        .collect()
}
