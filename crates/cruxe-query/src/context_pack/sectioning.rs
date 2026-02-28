use crate::search::SearchResult;

use super::ContextPackSection;

pub(super) fn assign_section(result: &SearchResult) -> (ContextPackSection, &'static str) {
    let path = result.path.to_ascii_lowercase();
    let kind = result
        .kind
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let chunk_type = result
        .chunk_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let snippet = result
        .snippet
        .as_deref()
        .unwrap_or_default()
        .trim_start()
        .to_ascii_lowercase();

    let is_definition = result.result_type == "symbol";
    let is_dependency = path.ends_with("cargo.toml")
        || path.ends_with("go.mod")
        || path.ends_with("go.sum")
        || path.ends_with("package.json")
        || path.ends_with("pnpm-lock.yaml")
        || path.ends_with("requirements.txt")
        || path.ends_with("pyproject.toml")
        || path.ends_with("pom.xml")
        || snippet.starts_with("use ")
        || snippet.starts_with("import ")
        || snippet.starts_with("from ")
        || kind.contains("import")
        || kind.contains("module");
    let is_test = path.contains("/tests/")
        || path.contains("/test/")
        || path.contains("_test.")
        || path.contains(".spec.")
        || path.ends_with("tests.rs");
    let is_config = path.ends_with(".toml")
        || path.ends_with(".yaml")
        || path.ends_with(".yml")
        || path.ends_with(".ini")
        || path.ends_with(".json")
        || path.ends_with("dockerfile")
        || path.contains(".github/workflows");
    let is_docs = path.ends_with(".md")
        || path.ends_with(".rst")
        || path.ends_with(".txt")
        || path.contains("/docs/")
        || path.contains("readme");
    let has_usage_signal = chunk_type.contains("usage")
        || chunk_type.contains("call")
        || chunk_type.contains("reference")
        || snippet.contains(" call ");
    let is_usage = (has_usage_signal && !is_test && !is_dependency && !is_config && !is_docs)
        || (result.result_type == "snippet"
            && !is_dependency
            && !is_test
            && !is_config
            && !is_docs);

    if is_definition {
        (ContextPackSection::Definitions, "definition")
    } else if is_usage {
        (ContextPackSection::Usages, "usage")
    } else if is_dependency {
        (ContextPackSection::Deps, "dependency")
    } else if is_test {
        (ContextPackSection::Tests, "test")
    } else if is_config {
        (ContextPackSection::Config, "config")
    } else if is_docs {
        (ContextPackSection::Docs, "docs")
    } else {
        (ContextPackSection::Usages, "default_usage")
    }
}
