pub const TAG_LANGUAGE_IDS: &[&str] = &["rust", "typescript", "python", "go"];

pub struct TagLanguageSpec {
    pub language: tree_sitter::Language,
    pub tags_query: &'static str,
}

pub fn tag_language_spec(language: &str) -> Option<TagLanguageSpec> {
    match language {
        "rust" => Some(TagLanguageSpec {
            language: tree_sitter_rust::LANGUAGE.into(),
            tags_query: tree_sitter_rust::TAGS_QUERY,
        }),
        "typescript" => Some(TagLanguageSpec {
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tags_query: tree_sitter_typescript::TAGS_QUERY,
        }),
        "python" => Some(TagLanguageSpec {
            language: tree_sitter_python::LANGUAGE.into(),
            tags_query: tree_sitter_python::TAGS_QUERY,
        }),
        "go" => Some(TagLanguageSpec {
            language: tree_sitter_go::LANGUAGE.into(),
            tags_query: tree_sitter_go::TAGS_QUERY,
        }),
        _ => None,
    }
}

pub fn parser_language(language: &str) -> Option<tree_sitter::Language> {
    tag_language_spec(language).map(|spec| spec.language)
}

pub fn combined_tags_query(language: &str) -> Option<String> {
    let spec = tag_language_spec(language)?;
    Some(format!(
        "{}{}",
        spec.tags_query,
        custom_query_extra(language)
    ))
}

fn custom_query_extra(language: &str) -> &'static str {
    match language {
        "rust" => {
            r#"
(const_item name: (identifier) @name) @definition.constant
(static_item name: (identifier) @name) @definition.variable
"#
        }
        "typescript" => {
            r#"
(function_declaration name: (identifier) @name) @definition.function
(class_declaration name: (type_identifier) @name) @definition.class
(method_definition name: (property_identifier) @name) @definition.method
(enum_declaration name: (identifier) @name) @definition.class
(type_alias_declaration name: (type_identifier) @name) @definition.class
(lexical_declaration (variable_declarator name: (identifier) @name)) @definition.variable
(variable_declaration (variable_declarator name: (identifier) @name)) @definition.variable
"#
        }
        "go" => {
            r#"
(const_declaration (const_spec name: (identifier) @name) @definition.constant)
(var_declaration (var_spec name: (identifier) @name) @definition.variable)
"#
        }
        "python" => "",
        _ => "",
    }
}
