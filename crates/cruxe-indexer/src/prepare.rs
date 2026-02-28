use crate::{call_extract, import_extract, languages, parser, snippet_extract, symbol_extract};
use cruxe_core::time::now_iso8601;
use cruxe_core::types::{CallEdge, FileRecord, SnippetRecord, SymbolRecord};

#[derive(Debug, Clone)]
pub struct SourceArtifacts {
    pub symbols: Vec<SymbolRecord>,
    pub snippets: Vec<SnippetRecord>,
    pub call_edges: Vec<CallEdge>,
    pub raw_imports: Vec<import_extract::RawImport>,
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct ArtifactBuildInput<'a> {
    pub content: &'a str,
    pub language: &'a str,
    pub source_path: &'a str,
    pub project_id: &'a str,
    pub ref_name: &'a str,
    pub source_layer: Option<&'a str>,
    pub include_imports: bool,
}

/// Build parser-derived artifacts for one file.
///
/// `include_imports` can be disabled in flows that do not need import edges
/// (e.g. overlay incremental sync currently manages call edges only).
pub fn build_source_artifacts(
    content: &str,
    language: &str,
    source_path: &str,
    project_id: &str,
    ref_name: &str,
    source_layer: Option<&str>,
    include_imports: bool,
) -> SourceArtifacts {
    let input = ArtifactBuildInput {
        content,
        language,
        source_path,
        project_id,
        ref_name,
        source_layer,
        include_imports,
    };
    build_source_artifacts_with_parser(input, |source, lang| {
        parser::parse_file(source, lang).map_err(|err| err.to_string())
    })
}

/// Same as [`build_source_artifacts`] but allows parser injection for tests.
pub fn build_source_artifacts_with_parser<F>(
    input: ArtifactBuildInput<'_>,
    mut parse_source: F,
) -> SourceArtifacts
where
    F: FnMut(&str, &str) -> Result<tree_sitter::Tree, String>,
{
    let ArtifactBuildInput {
        content,
        language,
        source_path,
        project_id,
        ref_name,
        source_layer,
        include_imports,
    } = input;

    let (parsed_tree, extracted, raw_imports, parse_error) =
        if parser::is_language_supported(language) {
            match parse_source(content, language) {
                Ok(tree) => {
                    let (extracted, diagnostics) =
                        languages::extract_symbols_with_diagnostics(&tree, content, language);
                    let raw_imports = if include_imports {
                        import_extract::extract_imports(&tree, content, language, source_path)
                    } else {
                        Vec::new()
                    };
                    let parse_error = diagnostics.had_tag_parse_error.then(|| {
                        "tree-sitter-tags reported parse errors; extracted symbols may be partial"
                            .to_string()
                    });
                    (Some(tree), extracted, raw_imports, parse_error)
                }
                Err(err) => (None, Vec::new(), Vec::new(), Some(err)),
            }
        } else {
            (None, Vec::new(), Vec::new(), None)
        };

    let symbols = symbol_extract::build_symbol_records(
        &extracted,
        project_id,
        ref_name,
        source_path,
        source_layer,
    );
    let snippets = snippet_extract::build_snippet_records(
        &extracted,
        project_id,
        ref_name,
        source_path,
        source_layer,
    );
    let call_edges = parsed_tree.as_ref().map_or_else(Vec::new, |tree| {
        call_extract::extract_call_edges_for_file(
            tree,
            content,
            language,
            source_path,
            &symbols,
            project_id,
            ref_name,
        )
    });

    SourceArtifacts {
        symbols,
        snippets,
        call_edges,
        raw_imports,
        parse_error,
    }
}

pub fn build_file_record(
    project_id: &str,
    ref_name: &str,
    path: &str,
    filename: &str,
    language: &str,
    content: &str,
) -> FileRecord {
    FileRecord {
        repo: project_id.to_string(),
        r#ref: ref_name.to_string(),
        commit: None,
        path: path.to_string(),
        filename: filename.to_string(),
        language: language.to_string(),
        content_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
        size_bytes: content.len() as u64,
        updated_at: now_iso8601(),
        content_head: Some(content.lines().take(20).collect::<Vec<_>>().join("\n")),
    }
}
