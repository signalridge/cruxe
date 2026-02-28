/// Canonical list of first-class indexable source languages.
///
/// These languages have full parser/extractor support in the index pipeline.
pub const INDEXABLE_SOURCE_LANGUAGES: [&str; 4] = ["rust", "typescript", "python", "go"];

/// Returns true if the language has full parser/extractor support.
pub fn is_indexable_source_language(language: &str) -> bool {
    INDEXABLE_SOURCE_LANGUAGES.contains(&language)
}

/// Returns the canonical first-class source language list.
pub fn supported_indexable_languages() -> &'static [&'static str] {
    &INDEXABLE_SOURCE_LANGUAGES
}

/// Returns true if the language should count as a "code language" for semantic
/// profile recommendation heuristics.
///
/// This intentionally includes `javascript` because `.js/.jsx` files are common
/// in mixed TS/JS repositories and still impact retrieval quality heuristics.
pub fn is_semantic_code_language(language: &str) -> bool {
    matches!(
        language,
        "rust" | "typescript" | "python" | "go" | "javascript"
    )
}

/// Detect language from file extension and return canonical language label.
///
/// The returned language can be broader than indexable languages to allow
/// metadata tracking for non-first-class languages.
pub fn detect_language_from_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("rust"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" => Some("javascript"),
        "py" | "pyi" => Some("python"),
        "go" => Some("go"),
        "java" => Some("java"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" => Some("cpp"),
        "rb" => Some("ruby"),
        "swift" => Some("swift"),
        "kt" | "kts" => Some("kotlin"),
        // Config/docs: not source code inputs for indexing pipeline.
        "toml" | "yaml" | "yml" | "json" | "md" | "txt" => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexable_language_set_matches_v1_scope() {
        assert_eq!(
            supported_indexable_languages(),
            &["rust", "typescript", "python", "go"]
        );
        assert!(is_indexable_source_language("rust"));
        assert!(!is_indexable_source_language("javascript"));
    }

    #[test]
    fn extension_detection_covers_supported_and_non_supported_languages() {
        assert_eq!(detect_language_from_extension("rs"), Some("rust"));
        assert_eq!(detect_language_from_extension("ts"), Some("typescript"));
        assert_eq!(detect_language_from_extension("js"), Some("javascript"));
        assert_eq!(detect_language_from_extension("md"), None);
    }
}
