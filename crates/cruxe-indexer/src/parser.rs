use cruxe_core::error::ParseError;

/// Parse a source file with tree-sitter and return the syntax tree.
pub fn parse_file(source: &str, language: &str) -> Result<tree_sitter::Tree, ParseError> {
    let mut parser = tree_sitter::Parser::new();

    let ts_language = get_language(language)?;
    parser
        .set_language(&ts_language)
        .map_err(|e| ParseError::GrammarNotAvailable {
            language: format!("{}: {}", language, e),
        })?;

    parser
        .parse(source, None)
        .ok_or_else(|| ParseError::TreeSitterFailed {
            path: format!("<{} source>", language),
        })
}

/// Get the tree-sitter language grammar for a given language.
pub fn get_language(language: &str) -> Result<tree_sitter::Language, ParseError> {
    match language {
        "rust" => Ok(tree_sitter_rust::LANGUAGE.into()),
        "typescript" => Ok(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "python" => Ok(tree_sitter_python::LANGUAGE.into()),
        "go" => Ok(tree_sitter_go::LANGUAGE.into()),
        _ => Err(ParseError::GrammarNotAvailable {
            language: language.into(),
        }),
    }
}

/// Check if a language grammar is available.
pub fn is_language_supported(language: &str) -> bool {
    matches!(language, "rust" | "typescript" | "python" | "go")
}

/// Get list of supported languages.
pub fn supported_languages() -> Vec<&'static str> {
    vec!["rust", "typescript", "python", "go"]
}
