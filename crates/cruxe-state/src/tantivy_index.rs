use cruxe_core::constants;
use cruxe_core::error::StateError;
use std::io::ErrorKind;
use std::path::Path;
use tantivy::Index;
use tantivy::Term;
use tantivy::collector::TopDocs;
use tantivy::query::TermQuery;
use tantivy::schema::*;
use tracing::info;

use crate::tokenizers;

/// Names for the three indices.
pub const SYMBOLS_INDEX: &str = "symbols";
pub const SNIPPETS_INDEX: &str = "snippets";
pub const FILES_INDEX: &str = "files";

const REQUIRED_SYMBOL_FIELDS: &[&str] = &[
    "file_key",
    "repo",
    "ref",
    "symbol_exact",
    "kind",
    "role",
    "language",
    "symbol_id",
    "symbol_stable_id",
    "path",
    "qualified_name",
    "line_start",
    "line_end",
];

const REQUIRED_SNIPPET_FIELDS: &[&str] = &[
    "file_key",
    "repo",
    "ref",
    "path",
    "language",
    "chunk_type",
    "content",
    "line_start",
    "line_end",
];

const REQUIRED_FILE_FIELDS: &[&str] = &[
    "file_key",
    "repo",
    "ref",
    "path",
    "filename",
    "language",
    "updated_at",
];

/// Create or open the symbols Tantivy index.
pub fn open_symbols_index(base_dir: &Path) -> Result<Index, StateError> {
    let dir = base_dir.join(SYMBOLS_INDEX);
    std::fs::create_dir_all(&dir).map_err(StateError::Io)?;

    let schema = build_symbols_schema();
    let index = open_or_create_index(&dir, schema, REQUIRED_SYMBOL_FIELDS)?;
    tokenizers::register_tokenizers(index.tokenizers());
    info!(?dir, "Symbols index opened");
    Ok(index)
}

/// Create or open the snippets Tantivy index.
pub fn open_snippets_index(base_dir: &Path) -> Result<Index, StateError> {
    let dir = base_dir.join(SNIPPETS_INDEX);
    std::fs::create_dir_all(&dir).map_err(StateError::Io)?;

    let schema = build_snippets_schema();
    let index = open_or_create_index(&dir, schema, REQUIRED_SNIPPET_FIELDS)?;
    tokenizers::register_tokenizers(index.tokenizers());
    info!(?dir, "Snippets index opened");
    Ok(index)
}

/// Create or open the files Tantivy index.
pub fn open_files_index(base_dir: &Path) -> Result<Index, StateError> {
    let dir = base_dir.join(FILES_INDEX);
    std::fs::create_dir_all(&dir).map_err(StateError::Io)?;

    let schema = build_files_schema();
    let index = open_or_create_index(&dir, schema, REQUIRED_FILE_FIELDS)?;
    tokenizers::register_tokenizers(index.tokenizers());
    info!(?dir, "Files index opened");
    Ok(index)
}

fn open_or_create_index(
    dir: &Path,
    schema: Schema,
    required_fields: &[&str],
) -> Result<Index, StateError> {
    let index = if dir_is_empty(dir)? {
        Index::create_in_dir(dir, schema).map_err(StateError::tantivy)?
    } else {
        Index::open_in_dir(dir).map_err(|e| {
            StateError::CorruptManifest(format!("failed to open index at {}: {}", dir.display(), e))
        })?
    };

    validate_required_fields(&index, required_fields)?;
    Ok(index)
}

fn open_existing_index(
    dir: &Path,
    required_fields: &[&str],
    index_name: &str,
) -> Result<Index, StateError> {
    if !dir.exists() {
        return Err(StateError::Io(std::io::Error::new(
            ErrorKind::NotFound,
            format!("{} index not found at {}", index_name, dir.display()),
        )));
    }

    let index = Index::open_in_dir(dir).map_err(|e| {
        StateError::CorruptManifest(format!(
            "failed to open {} index at {}: {}",
            index_name,
            dir.display(),
            e
        ))
    })?;
    validate_required_fields(&index, required_fields)?;
    tokenizers::register_tokenizers(index.tokenizers());
    Ok(index)
}

fn validate_required_fields(index: &Index, required_fields: &[&str]) -> Result<(), StateError> {
    let schema = index.schema();
    let missing: Vec<&str> = required_fields
        .iter()
        .copied()
        .filter(|name| schema.get_field(name).is_err())
        .collect();
    if !missing.is_empty() {
        tracing::warn!(
            missing_fields = ?missing,
            "index schema is incompatible with current required fields; reindex required"
        );
        return Err(StateError::SchemaMigrationRequired {
            current: 0,
            required: constants::SCHEMA_VERSION,
        });
    }
    Ok(())
}

fn dir_is_empty(path: &Path) -> Result<bool, StateError> {
    let mut entries = std::fs::read_dir(path).map_err(StateError::Io)?;
    Ok(entries.next().is_none())
}

/// Compute a composite key for Tantivy document deletion.
/// Format: `{repo}|{ref}|{path}` — used with `delete_term` for stale doc cleanup.
pub fn file_key(repo: &str, r#ref: &str, path: &str) -> String {
    format!("{}|{}|{}", repo, r#ref, path)
}

/// Build the symbols index schema per data-model.md.
fn build_symbols_schema() -> Schema {
    let mut builder = Schema::builder();

    // Composite key for delete_term (repo|ref|path)
    builder.add_text_field("file_key", STRING);

    // STRING fields (exact match)
    builder.add_text_field("repo", STRING | STORED);
    builder.add_text_field("ref", STRING | STORED);
    builder.add_text_field("commit", STORED);
    builder.add_text_field("symbol_exact", STRING | STORED);
    builder.add_text_field("kind", STRING | STORED);
    builder.add_text_field("role", STRING | STORED);
    builder.add_text_field("language", STRING | STORED);
    builder.add_text_field("visibility", STORED);
    builder.add_text_field("symbol_id", STRING | STORED);
    builder.add_text_field("symbol_stable_id", STRING | STORED);

    // TEXT fields with custom tokenizers
    let code_path_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("code_path")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    builder.add_text_field("path", code_path_options);

    let code_dotted_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("code_dotted")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    builder.add_text_field("qualified_name", code_dotted_options);

    let sig_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("code_signature")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    builder.add_text_field("signature", sig_options);

    // Full-text content
    builder.add_text_field("content", TEXT | STORED);

    // Numeric stored fields
    builder.add_u64_field("line_start", STORED);
    builder.add_u64_field("line_end", STORED);

    builder.build()
}

/// Build the snippets index schema per data-model.md.
fn build_snippets_schema() -> Schema {
    let mut builder = Schema::builder();

    // Composite key for delete_term (repo|ref|path)
    builder.add_text_field("file_key", STRING);

    builder.add_text_field("repo", STRING | STORED);
    builder.add_text_field("ref", STRING | STORED);
    builder.add_text_field("commit", STORED);
    builder.add_text_field("chunk_type", STRING | STORED);
    builder.add_text_field("language", STRING | STORED);

    let code_path_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("code_path")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    builder.add_text_field("path", code_path_options);

    let code_dotted_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("code_dotted")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    builder.add_text_field("imports", code_dotted_options);

    // Content with default tokenizer for full-text search
    builder.add_text_field("content", TEXT | STORED);

    builder.add_u64_field("line_start", STORED);
    builder.add_u64_field("line_end", STORED);

    builder.build()
}

/// Build the files index schema per data-model.md.
fn build_files_schema() -> Schema {
    let mut builder = Schema::builder();

    // Composite key for delete_term (repo|ref|path)
    builder.add_text_field("file_key", STRING);

    builder.add_text_field("repo", STRING | STORED);
    builder.add_text_field("ref", STRING | STORED);
    builder.add_text_field("commit", STORED);
    builder.add_text_field("filename", STRING | STORED);
    builder.add_text_field("language", STRING | STORED);
    builder.add_text_field("updated_at", STORED);

    let code_path_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("code_path")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    builder.add_text_field("path", code_path_options);

    builder.add_text_field("content_head", TEXT | STORED);

    builder.build()
}

/// Holder for all three indices.
pub struct IndexSet {
    pub symbols: Index,
    pub snippets: Index,
    pub files: Index,
}

impl IndexSet {
    /// Open all three indices from a project data directory.
    ///
    /// This resolves to `<data_dir>/base`.
    pub fn open(base_dir: &Path) -> Result<Self, StateError> {
        let base = base_dir.join("base");
        Self::open_at(&base)
    }

    /// Open all three indices from an explicit index root directory.
    ///
    /// `index_root` must be the directory that directly contains
    /// `symbols/`, `snippets/`, and `files/`.
    pub fn open_at(index_root: &Path) -> Result<Self, StateError> {
        Ok(Self {
            symbols: open_symbols_index(index_root)?,
            snippets: open_snippets_index(index_root)?,
            files: open_files_index(index_root)?,
        })
    }

    /// Open existing indices without creating new ones.
    ///
    /// Used by query paths to enforce explicit index compatibility handling.
    pub fn open_existing(base_dir: &Path) -> Result<Self, StateError> {
        let base = base_dir.join("base");
        Self::open_existing_at(&base)
    }

    /// Open existing indices from an explicit index root directory.
    ///
    /// Unlike `open_at`, this does not create missing indices.
    pub fn open_existing_at(index_root: &Path) -> Result<Self, StateError> {
        Ok(Self {
            symbols: open_existing_index(
                &index_root.join(SYMBOLS_INDEX),
                REQUIRED_SYMBOL_FIELDS,
                SYMBOLS_INDEX,
            )?,
            snippets: open_existing_index(
                &index_root.join(SNIPPETS_INDEX),
                REQUIRED_SNIPPET_FIELDS,
                SNIPPETS_INDEX,
            )?,
            files: open_existing_index(
                &index_root.join(FILES_INDEX),
                REQUIRED_FILE_FIELDS,
                FILES_INDEX,
            )?,
        })
    }
}

/// Result of a Tantivy health check for a single index.
#[derive(Debug, serde::Serialize)]
pub struct TantivyIndexHealth {
    pub name: &'static str,
    pub ok: bool,
    pub error: Option<String>,
}

/// Check health of all three Tantivy indices by attempting to open a reader.
pub fn check_tantivy_health(index_set: &IndexSet) -> Vec<TantivyIndexHealth> {
    let checks = [
        (SYMBOLS_INDEX, &index_set.symbols),
        (SNIPPETS_INDEX, &index_set.snippets),
        (FILES_INDEX, &index_set.files),
    ];

    checks
        .into_iter()
        .map(|(name, index)| match index.reader() {
            Ok(_) => TantivyIndexHealth {
                name,
                ok: true,
                error: None,
            },
            Err(e) => TantivyIndexHealth {
                name,
                ok: false,
                error: Some(e.to_string()),
            },
        })
        .collect()
}

/// Prewarm Tantivy indices by opening readers and touching segment metadata.
pub fn prewarm_indices(index_set: &IndexSet) -> Result<(), StateError> {
    for (name, index) in [
        (SYMBOLS_INDEX, &index_set.symbols),
        (SNIPPETS_INDEX, &index_set.snippets),
        (FILES_INDEX, &index_set.files),
    ] {
        let reader = index.reader().map_err(|e| {
            StateError::Tantivy(format!("Failed to open reader for {}: {}", name, e))
        })?;
        let searcher = reader.searcher();
        // Touch segment metadata to warm OS page cache
        let _total: u32 = searcher
            .segment_readers()
            .iter()
            .map(|s| s.num_docs())
            .sum();

        // Warm hot term lookup path for symbols index.
        if name == SYMBOLS_INDEX {
            let schema = index.schema();
            if let Ok(symbol_exact_field) = schema.get_field("symbol_exact") {
                for term in ["main", "init", "error"] {
                    let query = TermQuery::new(
                        Term::from_field_text(symbol_exact_field, term),
                        IndexRecordOption::Basic,
                    );
                    let _ = searcher
                        .search(&query, &TopDocs::with_limit(1))
                        .map_err(StateError::tantivy)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_open_symbols_index() {
        let dir = tempdir().unwrap();
        let index = open_symbols_index(dir.path()).unwrap();
        let schema = index.schema();
        assert!(schema.get_field("symbol_exact").is_ok());
        assert!(schema.get_field("qualified_name").is_ok());
        assert!(schema.get_field("path").is_ok());
        assert!(schema.get_field("kind").is_ok());
    }

    #[test]
    fn test_open_snippets_index() {
        let dir = tempdir().unwrap();
        let index = open_snippets_index(dir.path()).unwrap();
        let schema = index.schema();
        assert!(schema.get_field("content").is_ok());
        assert!(schema.get_field("chunk_type").is_ok());
    }

    #[test]
    fn test_open_files_index() {
        let dir = tempdir().unwrap();
        let index = open_files_index(dir.path()).unwrap();
        let schema = index.schema();
        assert!(schema.get_field("filename").is_ok());
        assert!(schema.get_field("path").is_ok());
    }

    #[test]
    fn test_index_set() {
        let dir = tempdir().unwrap();
        let base = dir.path().join("base");
        std::fs::create_dir_all(&base).unwrap();
        // IndexSet::open creates under base/
        let set = IndexSet::open(dir.path()).unwrap();
        assert!(set.symbols.schema().get_field("symbol_exact").is_ok());
    }

    #[test]
    fn test_index_set_open_at_explicit_root() {
        let dir = tempdir().unwrap();
        let overlay = dir.path().join("overlay").join("feat-auth");
        std::fs::create_dir_all(&overlay).unwrap();
        let set = IndexSet::open_at(&overlay).unwrap();
        assert!(set.symbols.schema().get_field("symbol_exact").is_ok());
        assert!(overlay.join("symbols").exists());
        assert!(overlay.join("snippets").exists());
        assert!(overlay.join("files").exists());
    }
}
