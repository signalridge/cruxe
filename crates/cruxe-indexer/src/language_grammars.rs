pub const TAG_LANGUAGE_IDS: &[&str] = &["rust", "typescript", "python", "go"];

pub struct TagLanguageSpec {
    pub language: tree_sitter::Language,
    pub tags_query: &'static str,
    pub locals_query: &'static str,
}

pub fn tag_language_spec(language: &str) -> Option<TagLanguageSpec> {
    match language {
        "rust" => Some(TagLanguageSpec {
            language: tree_sitter_rust::LANGUAGE.into(),
            tags_query: tree_sitter_rust::TAGS_QUERY,
            locals_query: "",
        }),
        "typescript" => Some(TagLanguageSpec {
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tags_query: tree_sitter_typescript::TAGS_QUERY,
            locals_query: tree_sitter_typescript::LOCALS_QUERY,
        }),
        "python" => Some(TagLanguageSpec {
            language: tree_sitter_python::LANGUAGE.into(),
            tags_query: tree_sitter_python::TAGS_QUERY,
            locals_query: "",
        }),
        "go" => Some(TagLanguageSpec {
            language: tree_sitter_go::LANGUAGE.into(),
            tags_query: tree_sitter_go::TAGS_QUERY,
            locals_query: "",
        }),
        _ => None,
    }
}

pub fn parser_language(language: &str) -> Option<tree_sitter::Language> {
    tag_language_spec(language).map(|spec| spec.language)
}
