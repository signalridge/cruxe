use crate::language_grammars;
use std::collections::HashMap;
use tree_sitter_tags::{TagsConfiguration, TagsContext};

// ---------------------------------------------------------------------------
// Custom query additions per language (appended to upstream TAGS_QUERY)
// ---------------------------------------------------------------------------

/// Rust: upstream tags.scm lacks const_item and static_item.
const RUST_EXTRA: &str = r#"
(const_item name: (identifier) @name) @definition.constant
(static_item name: (identifier) @name) @definition.variable
"#;

/// TypeScript: upstream tags.scm only covers .d.ts-style signatures.
/// We add concrete JS-inherited constructs for real source files.
const TYPESCRIPT_EXTRA: &str = r#"
(function_declaration name: (identifier) @name) @definition.function
(class_declaration name: (type_identifier) @name) @definition.class
(method_definition name: (property_identifier) @name) @definition.method
(enum_declaration name: (identifier) @name) @definition.class
(type_alias_declaration name: (type_identifier) @name) @definition.class
(lexical_declaration (variable_declarator name: (identifier) @name)) @definition.variable
(variable_declaration (variable_declarator name: (identifier) @name)) @definition.variable
"#;

/// Python upstream tags.scm has sufficient coverage; no extras needed.
const PYTHON_EXTRA: &str = "";
/// Go upstream tags.scm captures const/var names but without @definition.*,
/// so tree-sitter-tags would otherwise skip them.
const GO_EXTRA: &str = r#"
(const_declaration (const_spec name: (identifier) @name) @definition.constant)
(var_declaration (var_spec name: (identifier) @name) @definition.variable)
"#;

/// Build all language tag configurations.
fn build_configs() -> HashMap<&'static str, TagsConfiguration> {
    let mut m = HashMap::new();

    for &language in language_grammars::TAG_LANGUAGE_IDS {
        let Some(spec) = language_grammars::tag_language_spec(language) else {
            continue;
        };
        let query = format!("{}{}", spec.tags_query, custom_query_extra(language));
        let config = TagsConfiguration::new(spec.language, &query, spec.locals_query)
            .unwrap_or_else(|_| panic!("{language} tags config"));
        m.insert(language, config);
    }

    m
}

fn custom_query_extra(language: &str) -> &'static str {
    match language {
        "rust" => RUST_EXTRA,
        "typescript" => TYPESCRIPT_EXTRA,
        "python" => PYTHON_EXTRA,
        "go" => GO_EXTRA,
        _ => "",
    }
}

// Thread-local state: configs and context in separate RefCells to avoid borrow conflicts.
// TagsConfiguration in tree-sitter-tags 0.24 doesn't implement Send/Sync
// (fixed in 0.26.x), so we use thread_local instead of OnceLock.
thread_local! {
    static CONFIGS: std::cell::RefCell<HashMap<&'static str, TagsConfiguration>> =
        std::cell::RefCell::new(build_configs());
    static CONTEXT: std::cell::RefCell<TagsContext> =
        std::cell::RefCell::new(TagsContext::new());
}

/// Run a closure with the thread-local configs and TagsContext.
pub fn with_tags<F, R>(f: F) -> R
where
    F: FnOnce(&HashMap<&'static str, TagsConfiguration>, &mut TagsContext) -> R,
{
    CONFIGS.with(|configs| {
        CONTEXT.with(|ctx| {
            let configs = configs.borrow();
            let mut ctx = ctx.borrow_mut();
            f(&configs, &mut ctx)
        })
    })
}
