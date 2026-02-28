use super::ExtractedSymbol;
use super::generic_mapper;
use super::tag_registry;
use tracing::debug;
use tree_sitter_tags::Tag;

#[derive(Debug, Clone, Copy, Default)]
pub struct TagExtractionDiagnostics {
    pub had_parse_error: bool,
}

/// Extract symbols from source using tree-sitter-tags + language enricher.
///
/// 1. Generate tags (tree-sitter-tags parses internally).
/// 2. Use caller-provided tree for enrichment (parent walking, visibility, kind disambiguation).
/// 3. Map each definition tag to ExtractedSymbol via the enricher.
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
    let source_bytes = source.as_bytes();

    // Step 1: Collect tags via thread-local configs + context.
    let (tags, had_parse_error): (Vec<(Tag, String)>, bool) =
        tag_registry::with_tags(|configs, ctx| {
            let Some(config) = configs.get(language) else {
                return (Vec::new(), false);
            };
            let Ok((iter, has_error)) = ctx.generate_tags(config, source_bytes, None) else {
                return (Vec::new(), true);
            };
            (
                iter.filter_map(|r| r.ok())
                    .filter(|t| t.is_definition)
                    .map(|t| {
                        let kind_name = config.syntax_type_name(t.syntax_type_id).to_string();
                        (t, kind_name)
                    })
                    .collect(),
                has_error,
            )
        });
    if had_parse_error {
        debug!(
            language,
            "tree-sitter-tags reported parse errors; extracted symbols may be partial"
        );
    }

    // Step 3: Map definition tags to ExtractedSymbol.
    let symbols = tags
        .iter()
        .filter_map(|(tag, kind_name)| map_tag_to_symbol(tag, kind_name, source, tree, language))
        .collect();

    (symbols, TagExtractionDiagnostics { had_parse_error })
}

fn map_tag_to_symbol(
    tag: &Tag,
    tag_kind: &str,
    source: &str,
    tree: &tree_sitter::Tree,
    language: &str,
) -> Option<ExtractedSymbol> {
    let name = source.get(tag.name_range.clone())?.to_string();
    let body = source.get(tag.range.clone()).map(String::from);

    // Find the tree-sitter node at this position for enrichment.
    let node = tree
        .root_node()
        .descendant_for_byte_range(tag.range.start, tag.range.end);

    let parent_name = node.and_then(|n| generic_mapper::find_parent_scope(n, source));
    let has_parent = parent_name.is_some();

    let kind = generic_mapper::map_tag_kind(tag_kind, has_parent, node.map(|n| n.kind()))?;
    let signature = generic_mapper::extract_signature(kind, source, tag.line_range.clone());
    let visibility = None;

    let qualified_name = match &parent_name {
        Some(p) => format!(
            "{}{}{}",
            p,
            generic_mapper::separator_for_language(language),
            name
        ),
        None => name.clone(),
    };

    // Prefer AST node range for definition coverage (function body lines, etc.)
    // and only fall back to tag span when the node cannot be resolved.
    let (line_start, line_end) = if let Some(n) = node {
        (
            n.start_position().row as u32 + 1,
            n.end_position().row as u32 + 1,
        )
    } else {
        // tag span is already 0-indexed row.
        (tag.span.start.row as u32 + 1, tag.span.end.row as u32 + 1)
    };

    Some(ExtractedSymbol {
        name,
        qualified_name,
        kind,
        language: language.to_string(),
        signature,
        line_start,
        line_end,
        visibility,
        parent_name,
        body,
    })
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
