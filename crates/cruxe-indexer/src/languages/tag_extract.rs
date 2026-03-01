use super::ExtractedSymbol;
use super::generic_mapper;
use crate::language_grammars;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use streaming_iterator::StreamingIterator;
use tracing::debug;
use tree_sitter::{Query, QueryCapture, QueryCursor, QueryMatch};

#[derive(Debug, Clone, Copy, Default)]
pub struct TagExtractionDiagnostics {
    pub had_parse_error: bool,
}

thread_local! {
    static QUERY_CACHE: RefCell<HashMap<&'static str, Query>> = RefCell::new(HashMap::new());
}

/// Extract symbols from source using the pre-parsed tree and tree-sitter query execution.
///
/// This pipeline runs on the caller-provided tree (single parse):
/// 1. Execute combined `TAGS_QUERY + extras` with `QueryCursor`.
/// 2. Resolve `@definition.*` + `@name` captures into symbols.
/// 3. Apply language-aware enrichment (parent scope, kind disambiguation).
pub fn extract_symbols_via_tags(
    tree: &tree_sitter::Tree,
    source: &str,
    language: &str,
) -> Vec<ExtractedSymbol> {
    extract_symbols_via_tags_with_diagnostics(tree, source, language).0
}

pub fn extract_symbols_via_tags_with_diagnostics(
    tree: &tree_sitter::Tree,
    source: &str,
    language: &str,
) -> (Vec<ExtractedSymbol>, TagExtractionDiagnostics) {
    let had_parse_error = tree.root_node().has_error();
    if had_parse_error {
        debug!(
            language,
            "tree-sitter parser reported syntax errors; extracted symbols may be partial"
        );
    }

    let Some(language_id) = canonical_tag_language(language) else {
        return (Vec::new(), TagExtractionDiagnostics { had_parse_error });
    };

    let symbols = match with_compiled_query(language_id, |query| {
        collect_definition_symbols(query, tree, source, language_id)
    }) {
        Ok(symbols) => symbols,
        Err(err) => {
            debug!(
                language = language_id,
                error = %err,
                "failed to build or execute symbol query"
            );
            return (
                Vec::new(),
                TagExtractionDiagnostics {
                    had_parse_error: true,
                },
            );
        }
    };

    (symbols, TagExtractionDiagnostics { had_parse_error })
}

fn canonical_tag_language(language: &str) -> Option<&'static str> {
    match language {
        "rust" => Some("rust"),
        "typescript" => Some("typescript"),
        "python" => Some("python"),
        "go" => Some("go"),
        _ => None,
    }
}

fn with_compiled_query<R, F>(language: &'static str, f: F) -> Result<R, String>
where
    F: FnOnce(&Query) -> R,
{
    QUERY_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(language) {
            let ts_language = language_grammars::parser_language(language)
                .ok_or_else(|| format!("unsupported language: {language}"))?;
            let query_source = language_grammars::combined_tags_query(language)
                .ok_or_else(|| format!("missing query for language: {language}"))?;
            let query = Query::new(&ts_language, &query_source)
                .map_err(|err| format!("query compile failed for {language}: {err}"))?;
            cache.insert(language, query);
        }
        let query = cache
            .get(language)
            .ok_or_else(|| format!("query cache missing language: {language}"))?;
        Ok(f(query))
    })
}

fn collect_definition_symbols(
    query: &Query,
    tree: &tree_sitter::Tree,
    source: &str,
    language: &str,
) -> Vec<ExtractedSymbol> {
    let capture_names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut seen = HashSet::<(usize, usize, usize, usize, cruxe_core::types::SymbolKind)>::new();
    let mut symbols = Vec::new();
    let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());

    while let Some(query_match) = matches.next() {
        let Some((name_capture, definition_capture)) =
            select_definition_captures(query_match, capture_names)
        else {
            continue;
        };

        let Some(definition_capture_name) = capture_names
            .get(definition_capture.index as usize)
            .copied()
        else {
            continue;
        };
        let Some(tag_kind) = definition_capture_name.strip_prefix("definition.") else {
            continue;
        };

        let Some(symbol) =
            map_capture_to_symbol(name_capture, definition_capture, tag_kind, source, language)
        else {
            continue;
        };

        let dedupe_key = (
            name_capture.node.start_byte(),
            name_capture.node.end_byte(),
            definition_capture.node.start_byte(),
            definition_capture.node.end_byte(),
            symbol.kind,
        );
        if !seen.insert(dedupe_key) {
            continue;
        }

        symbols.push(symbol);
    }

    symbols.sort_by(|a, b| {
        a.line_start
            .cmp(&b.line_start)
            .then_with(|| a.line_end.cmp(&b.line_end))
            .then_with(|| a.name.cmp(&b.name))
    });
    symbols
}

fn select_definition_captures<'cursor, 'tree: 'cursor>(
    query_match: &'cursor QueryMatch<'cursor, 'tree>,
    capture_names: &[&str],
) -> Option<(QueryCapture<'tree>, QueryCapture<'tree>)> {
    let mut name_capture = None;
    let mut definition_capture = None;

    for capture in query_match.captures.iter().copied() {
        let capture_name = capture_names
            .get(capture.index as usize)
            .copied()
            .unwrap_or("");
        if capture_name == "name" && name_capture.is_none() {
            name_capture = Some(capture);
            continue;
        }
        if definition_capture.is_none() && capture_name.starts_with("definition.") {
            definition_capture = Some(capture);
        }
    }

    let name_capture = name_capture?;
    let definition_capture = definition_capture?;
    Some((name_capture, definition_capture))
}

fn map_capture_to_symbol(
    name_capture: QueryCapture<'_>,
    definition_capture: QueryCapture<'_>,
    tag_kind: &str,
    source: &str,
    language: &str,
) -> Option<ExtractedSymbol> {
    let name = source.get(name_capture.node.byte_range())?.to_string();
    let definition_node = definition_capture.node;
    let definition_range = definition_node.byte_range();
    let body = source.get(definition_range.clone()).map(String::from);

    let parent_name = generic_mapper::find_parent_scope(definition_node, source);
    let has_parent = parent_name.is_some();
    let kind = generic_mapper::map_tag_kind(tag_kind, has_parent, Some(definition_node.kind()))?;
    let signature = generic_mapper::extract_signature(
        kind,
        source,
        range_from_node_or_default(source, definition_range.clone()),
    );
    let visibility = None;

    let qualified_name = match &parent_name {
        Some(parent) => format!(
            "{}{}{}",
            parent,
            generic_mapper::separator_for_language(language),
            name
        ),
        None => name.clone(),
    };

    Some(ExtractedSymbol {
        name,
        qualified_name,
        kind,
        language: language.to_string(),
        signature,
        line_start: definition_node.start_position().row as u32 + 1,
        line_end: definition_node.end_position().row as u32 + 1,
        visibility,
        parent_name,
        body,
    })
}

fn range_from_node_or_default(source: &str, range: Range<usize>) -> Range<usize> {
    let start = range.start.min(source.len());
    let end = range.end.min(source.len()).max(start);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    #[test]
    fn signature_is_only_emitted_for_callable_symbols() {
        let source = r#"
struct Foo {
    value: i32
}

fn run() {}
"#;
        let tree = parse_file(source, "rust").expect("parse rust");
        let symbols = extract_symbols_via_tags(&tree, source, "rust");

        let struct_symbol = symbols.iter().find(|s| s.name == "Foo").expect("Foo");
        assert_eq!(struct_symbol.signature, None);

        let fn_symbol = symbols.iter().find(|s| s.name == "run").expect("run");
        assert!(fn_symbol.signature.is_some(), "expected callable signature");
    }

    #[test]
    fn diagnostics_flag_partial_parse_errors() {
        let source = "fn broken( {";
        let tree = parse_file(source, "rust").expect("parse rust");
        let (_symbols, diagnostics) =
            extract_symbols_via_tags_with_diagnostics(&tree, source, "rust");
        assert!(diagnostics.had_parse_error);
    }

    #[test]
    fn multiline_function_uses_ast_range_for_line_end() {
        let source = r#"
fn outer() {
    inner();
}

fn inner() {}
"#;
        let tree = parse_file(source, "rust").expect("parse rust");
        let symbols = extract_symbols_via_tags(&tree, source, "rust");
        let outer = symbols.iter().find(|s| s.name == "outer").expect("outer");
        assert!(
            outer.line_end > outer.line_start,
            "line range should cover multiline function body"
        );
    }
}
