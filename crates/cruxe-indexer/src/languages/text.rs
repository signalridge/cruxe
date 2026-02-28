/// Extract text from a tree-sitter node using safe range access.
///
/// Call-site/import extractors keep owned strings to avoid lifetime plumbing.
pub(crate) fn node_text_owned(node: tree_sitter::Node, source: &str) -> String {
    source.get(node.byte_range()).unwrap_or("").to_string()
}
