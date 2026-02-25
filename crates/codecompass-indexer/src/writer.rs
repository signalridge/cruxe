use crate::import_extract::{self, RawImport};
use codecompass_core::error::StateError;
use codecompass_core::time::now_iso8601;
use codecompass_core::types::{FileRecord, SnippetRecord, SymbolRecord};
use codecompass_state::tantivy_index::{self, IndexSet};
use codecompass_state::{edges, manifest, symbols};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use tantivy::{IndexWriter, Term, doc};
use tracing::{debug, info};

/// Destination for index writes in VCS mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteTarget<'a> {
    /// Shared immutable default branch index at `<data_dir>/base`.
    Base,
    /// Ref-scoped overlay at `<data_dir>/overlay/<normalized_ref>`.
    Overlay { branch: &'a str },
    /// Sync-local staging area at `<data_dir>/staging/<sync_id>`.
    Staging { sync_id: &'a str },
}

impl WriteTarget<'_> {
    /// Resolve the concrete index root path for this write target.
    pub fn index_root(self, data_dir: &Path) -> PathBuf {
        match self {
            Self::Base => data_dir.join("base"),
            Self::Overlay { branch } => crate::overlay::overlay_dir_for_ref(data_dir, branch),
            Self::Staging { sync_id } => crate::staging::staging_dir(data_dir, sync_id),
        }
    }
}

/// Open/create an index set for the specified write target.
pub fn open_index_set_for_target(
    data_dir: &Path,
    target: WriteTarget<'_>,
) -> Result<IndexSet, StateError> {
    let root = target.index_root(data_dir);
    IndexSet::open_at(&root)
}

/// Open an existing index set for the specified write target.
pub fn open_existing_index_set_for_target(
    data_dir: &Path,
    target: WriteTarget<'_>,
) -> Result<IndexSet, StateError> {
    let root = target.index_root(data_dir);
    IndexSet::open_existing_at(&root)
}

/// Batch writer that holds a single IndexWriter per index.
/// Documents are accumulated and committed together for performance.
pub struct BatchWriter {
    symbol_writer: IndexWriter,
    snippet_writer: IndexWriter,
    file_writer: IndexWriter,
}

impl BatchWriter {
    /// Create a new batch writer. Allocates one IndexWriter per index (50MB buffer each).
    pub fn new(index_set: &IndexSet) -> Result<Self, StateError> {
        Ok(Self {
            symbol_writer: index_set
                .symbols
                .writer(50_000_000)
                .map_err(StateError::tantivy)?,
            snippet_writer: index_set
                .snippets
                .writer(50_000_000)
                .map_err(StateError::tantivy)?,
            file_writer: index_set
                .files
                .writer(50_000_000)
                .map_err(StateError::tantivy)?,
        })
    }

    /// Delete all stale Tantivy documents for a file before re-indexing.
    /// Uses the `file_key` STRING field (`repo|ref|path`) for efficient `delete_term`.
    pub fn delete_file_docs(&self, index_set: &IndexSet, repo: &str, r#ref: &str, path: &str) {
        let key = tantivy_index::file_key(repo, r#ref, path);

        if let Ok(f) = index_set.symbols.schema().get_field("file_key") {
            self.symbol_writer
                .delete_term(Term::from_field_text(f, &key));
        }
        if let Ok(f) = index_set.snippets.schema().get_field("file_key") {
            self.snippet_writer
                .delete_term(Term::from_field_text(f, &key));
        }
        if let Ok(f) = index_set.files.schema().get_field("file_key") {
            self.file_writer.delete_term(Term::from_field_text(f, &key));
        }
    }

    /// Delete ALL documents from all indices (used for `--force` full re-index).
    pub fn delete_all(&self) -> Result<(), StateError> {
        self.symbol_writer
            .delete_all_documents()
            .map_err(StateError::tantivy)?;
        self.snippet_writer
            .delete_all_documents()
            .map_err(StateError::tantivy)?;
        self.file_writer
            .delete_all_documents()
            .map_err(StateError::tantivy)?;
        Ok(())
    }

    /// Delete all Tantivy documents for a specific ref.
    /// Scoped deletion for `--force` mode in multi-ref environments —
    /// avoids wiping documents belonging to other refs.
    pub fn delete_ref_docs(&self, index_set: &IndexSet, r#ref: &str) {
        if let Ok(f) = index_set.symbols.schema().get_field("ref") {
            self.symbol_writer
                .delete_term(Term::from_field_text(f, r#ref));
        }
        if let Ok(f) = index_set.snippets.schema().get_field("ref") {
            self.snippet_writer
                .delete_term(Term::from_field_text(f, r#ref));
        }
        if let Ok(f) = index_set.files.schema().get_field("ref") {
            self.file_writer
                .delete_term(Term::from_field_text(f, r#ref));
        }
    }

    /// Add symbol documents to the batch.
    pub fn add_symbols(
        &self,
        index: &tantivy::Index,
        symbols: &[SymbolRecord],
    ) -> Result<(), StateError> {
        let schema = index.schema();
        let f = |name: &str| schema.get_field(name).map_err(StateError::tantivy);
        let fk = f("file_key")?;
        let f_repo = f("repo")?;
        let f_ref = f("ref")?;
        let f_commit = f("commit")?;
        let f_symbol_exact = f("symbol_exact")?;
        let f_kind = f("kind")?;
        let f_language = f("language")?;
        let f_visibility = f("visibility")?;
        let f_symbol_id = f("symbol_id")?;
        let f_symbol_stable_id = f("symbol_stable_id")?;
        let f_path = f("path")?;
        let f_qualified_name = f("qualified_name")?;
        let f_signature = f("signature")?;
        let f_content = f("content")?;
        let f_line_start = f("line_start")?;
        let f_line_end = f("line_end")?;

        for sym in symbols {
            let key = tantivy_index::file_key(&sym.repo, &sym.r#ref, &sym.path);
            let doc = doc!(
                fk => key.as_str(),
                f_repo => sym.repo.as_str(),
                f_ref => sym.r#ref.as_str(),
                f_commit => sym.commit.as_deref().unwrap_or(""),
                f_symbol_exact => sym.name.as_str(),
                f_kind => sym.kind.as_str(),
                f_language => sym.language.as_str(),
                f_visibility => sym.visibility.as_deref().unwrap_or(""),
                f_symbol_id => sym.symbol_id.as_str(),
                f_symbol_stable_id => sym.symbol_stable_id.as_str(),
                f_path => sym.path.as_str(),
                f_qualified_name => sym.qualified_name.as_str(),
                f_signature => sym.signature.as_deref().unwrap_or(""),
                f_content => sym.content.as_deref().unwrap_or(""),
                f_line_start => sym.line_start as u64,
                f_line_end => sym.line_end as u64
            );
            self.symbol_writer
                .add_document(doc)
                .map_err(StateError::tantivy)?;
        }
        Ok(())
    }

    /// Add snippet documents to the batch.
    pub fn add_snippets(
        &self,
        index: &tantivy::Index,
        snippets: &[SnippetRecord],
    ) -> Result<(), StateError> {
        let schema = index.schema();
        let f = |name: &str| schema.get_field(name).map_err(StateError::tantivy);
        let fk = f("file_key")?;
        let f_repo = f("repo")?;
        let f_ref = f("ref")?;
        let f_commit = f("commit")?;
        let f_path = f("path")?;
        let f_language = f("language")?;
        let f_chunk_type = f("chunk_type")?;
        let f_imports = f("imports")?;
        let f_content = f("content")?;
        let f_line_start = f("line_start")?;
        let f_line_end = f("line_end")?;

        for snip in snippets {
            let key = tantivy_index::file_key(&snip.repo, &snip.r#ref, &snip.path);
            let doc = doc!(
                fk => key.as_str(),
                f_repo => snip.repo.as_str(),
                f_ref => snip.r#ref.as_str(),
                f_commit => snip.commit.as_deref().unwrap_or(""),
                f_path => snip.path.as_str(),
                f_language => snip.language.as_str(),
                f_chunk_type => snip.chunk_type.as_str(),
                f_imports => snip.imports.as_deref().unwrap_or(""),
                f_content => snip.content.as_str(),
                f_line_start => snip.line_start as u64,
                f_line_end => snip.line_end as u64
            );
            self.snippet_writer
                .add_document(doc)
                .map_err(StateError::tantivy)?;
        }
        Ok(())
    }

    /// Add a file document to the batch.
    pub fn add_file(&self, index: &tantivy::Index, file: &FileRecord) -> Result<(), StateError> {
        let schema = index.schema();
        let f = |name: &str| schema.get_field(name).map_err(StateError::tantivy);
        let fk = f("file_key")?;
        let f_repo = f("repo")?;
        let f_ref = f("ref")?;
        let f_commit = f("commit")?;
        let f_path = f("path")?;
        let f_filename = f("filename")?;
        let f_language = f("language")?;
        let f_updated_at = f("updated_at")?;
        let f_content_head = f("content_head")?;

        let key = tantivy_index::file_key(&file.repo, &file.r#ref, &file.path);
        let doc = doc!(
            fk => key.as_str(),
            f_repo => file.repo.as_str(),
            f_ref => file.r#ref.as_str(),
            f_commit => file.commit.as_deref().unwrap_or(""),
            f_path => file.path.as_str(),
            f_filename => file.filename.as_str(),
            f_language => file.language.as_str(),
            f_updated_at => file.updated_at.as_str(),
            f_content_head => file.content_head.as_deref().unwrap_or("")
        );
        self.file_writer
            .add_document(doc)
            .map_err(StateError::tantivy)?;
        Ok(())
    }

    /// Write symbols to SQLite and manifest entry for a file.
    pub fn write_sqlite(
        &self,
        conn: &Connection,
        symbols: &[SymbolRecord],
        file_record: &FileRecord,
        mtime_ns: Option<i64>,
    ) -> Result<(), StateError> {
        for sym in symbols {
            symbols::insert_symbol(conn, sym)?;
        }

        manifest::upsert_manifest(
            conn,
            &manifest::ManifestEntry {
                repo: file_record.repo.clone(),
                r#ref: file_record.r#ref.clone(),
                path: file_record.path.clone(),
                content_hash: file_record.content_hash.clone(),
                size_bytes: file_record.size_bytes,
                mtime_ns,
                language: Some(file_record.language.clone()),
                indexed_at: now_iso8601(),
            },
        )?;

        Ok(())
    }

    /// Replace import edges for a file atomically.
    ///
    /// This deletes all existing edges from the file-scoped pseudo source id and
    /// inserts a freshly resolved set derived from `raw_imports`.
    pub fn replace_import_edges_for_file(
        &self,
        conn: &Connection,
        repo: &str,
        ref_name: &str,
        file_path: &str,
        raw_imports: Vec<RawImport>,
    ) -> Result<(), StateError> {
        self::replace_import_edges_for_file(conn, repo, ref_name, file_path, raw_imports)
    }

    /// Commit all three index writers at once.
    pub fn commit(mut self) -> Result<(), StateError> {
        self.symbol_writer.commit().map_err(StateError::tantivy)?;
        self.snippet_writer.commit().map_err(StateError::tantivy)?;
        self.file_writer.commit().map_err(StateError::tantivy)?;
        info!("All indices committed");
        Ok(())
    }
}

/// Write all records for a single file to both Tantivy and SQLite.
/// Legacy per-file API used by tests and MCP server fixture builder.
pub fn write_file_records(
    index_set: &IndexSet,
    conn: &Connection,
    symbols: &[SymbolRecord],
    snippets: &[SnippetRecord],
    file_record: &FileRecord,
) -> Result<(), StateError> {
    write_symbols_to_tantivy(&index_set.symbols, symbols)?;

    for sym in symbols {
        symbols::insert_symbol(conn, sym)?;
    }

    write_snippets_to_tantivy(&index_set.snippets, snippets)?;
    write_file_to_tantivy(&index_set.files, file_record)?;

    let now = now_iso8601();
    manifest::upsert_manifest(
        conn,
        &manifest::ManifestEntry {
            repo: file_record.repo.clone(),
            r#ref: file_record.r#ref.clone(),
            path: file_record.path.clone(),
            content_hash: file_record.content_hash.clone(),
            size_bytes: file_record.size_bytes,
            mtime_ns: None,
            language: Some(file_record.language.clone()),
            indexed_at: now,
        },
    )?;

    debug!(path = %file_record.path, symbols = symbols.len(), snippets = snippets.len(), "Wrote file records");
    Ok(())
}

/// Replace import edges for a file atomically within a transaction.
pub fn replace_import_edges_for_file(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    file_path: &str,
    raw_imports: Vec<RawImport>,
) -> Result<(), StateError> {
    let source_edge_id = import_extract::source_symbol_id_for_path(file_path);
    let resolved = import_extract::resolve_imports(conn, raw_imports, repo, ref_name)?;
    edges::replace_edges_for_file(
        conn,
        repo,
        ref_name,
        vec![source_edge_id.as_str()],
        resolved,
    )?;
    Ok(())
}

fn write_symbols_to_tantivy(
    index: &tantivy::Index,
    symbols: &[SymbolRecord],
) -> Result<(), StateError> {
    let schema = index.schema();
    let f = |name: &str| schema.get_field(name).map_err(StateError::tantivy);
    let f_repo = f("repo")?;
    let f_ref = f("ref")?;
    let f_commit = f("commit")?;
    let f_symbol_exact = f("symbol_exact")?;
    let f_kind = f("kind")?;
    let f_language = f("language")?;
    let f_visibility = f("visibility")?;
    let f_symbol_id = f("symbol_id")?;
    let f_symbol_stable_id = f("symbol_stable_id")?;
    let f_path = f("path")?;
    let f_qualified_name = f("qualified_name")?;
    let f_signature = f("signature")?;
    let f_content = f("content")?;
    let f_line_start = f("line_start")?;
    let f_line_end = f("line_end")?;
    let f_file_key = schema.get_field("file_key").ok();

    let mut writer = index.writer(50_000_000).map_err(StateError::tantivy)?;

    for sym in symbols {
        let mut doc = doc!(
            f_repo => sym.repo.as_str(),
            f_ref => sym.r#ref.as_str(),
            f_commit => sym.commit.as_deref().unwrap_or(""),
            f_symbol_exact => sym.name.as_str(),
            f_kind => sym.kind.as_str(),
            f_language => sym.language.as_str(),
            f_visibility => sym.visibility.as_deref().unwrap_or(""),
            f_symbol_id => sym.symbol_id.as_str(),
            f_symbol_stable_id => sym.symbol_stable_id.as_str(),
            f_path => sym.path.as_str(),
            f_qualified_name => sym.qualified_name.as_str(),
            f_signature => sym.signature.as_deref().unwrap_or(""),
            f_content => sym.content.as_deref().unwrap_or(""),
            f_line_start => sym.line_start as u64,
            f_line_end => sym.line_end as u64
        );
        if let Some(fk) = f_file_key {
            let key = tantivy_index::file_key(&sym.repo, &sym.r#ref, &sym.path);
            doc.add_text(fk, &key);
        }
        writer.add_document(doc).map_err(StateError::tantivy)?;
    }

    writer.commit().map_err(StateError::tantivy)?;
    Ok(())
}

fn write_snippets_to_tantivy(
    index: &tantivy::Index,
    snippets: &[SnippetRecord],
) -> Result<(), StateError> {
    let schema = index.schema();
    let f = |name: &str| schema.get_field(name).map_err(StateError::tantivy);
    let f_repo = f("repo")?;
    let f_ref = f("ref")?;
    let f_commit = f("commit")?;
    let f_path = f("path")?;
    let f_language = f("language")?;
    let f_chunk_type = f("chunk_type")?;
    let f_imports = f("imports")?;
    let f_content = f("content")?;
    let f_line_start = f("line_start")?;
    let f_line_end = f("line_end")?;
    let f_file_key = schema.get_field("file_key").ok();

    let mut writer = index.writer(50_000_000).map_err(StateError::tantivy)?;

    for snip in snippets {
        let mut doc = doc!(
            f_repo => snip.repo.as_str(),
            f_ref => snip.r#ref.as_str(),
            f_commit => snip.commit.as_deref().unwrap_or(""),
            f_path => snip.path.as_str(),
            f_language => snip.language.as_str(),
            f_chunk_type => snip.chunk_type.as_str(),
            f_imports => snip.imports.as_deref().unwrap_or(""),
            f_content => snip.content.as_str(),
            f_line_start => snip.line_start as u64,
            f_line_end => snip.line_end as u64
        );
        if let Some(fk) = f_file_key {
            let key = tantivy_index::file_key(&snip.repo, &snip.r#ref, &snip.path);
            doc.add_text(fk, &key);
        }
        writer.add_document(doc).map_err(StateError::tantivy)?;
    }

    writer.commit().map_err(StateError::tantivy)?;
    Ok(())
}

fn write_file_to_tantivy(index: &tantivy::Index, file: &FileRecord) -> Result<(), StateError> {
    let schema = index.schema();
    let f = |name: &str| schema.get_field(name).map_err(StateError::tantivy);
    let f_repo = f("repo")?;
    let f_ref = f("ref")?;
    let f_commit = f("commit")?;
    let f_path = f("path")?;
    let f_filename = f("filename")?;
    let f_language = f("language")?;
    let f_updated_at = f("updated_at")?;
    let f_content_head = f("content_head")?;
    let f_file_key = schema.get_field("file_key").ok();

    let mut writer = index.writer(50_000_000).map_err(StateError::tantivy)?;

    let mut doc = doc!(
        f_repo => file.repo.as_str(),
        f_ref => file.r#ref.as_str(),
        f_commit => file.commit.as_deref().unwrap_or(""),
        f_path => file.path.as_str(),
        f_filename => file.filename.as_str(),
        f_language => file.language.as_str(),
        f_updated_at => file.updated_at.as_str(),
        f_content_head => file.content_head.as_deref().unwrap_or("")
    );
    if let Some(fk) = f_file_key {
        let key = tantivy_index::file_key(&file.repo, &file.r#ref, &file.path);
        doc.add_text(fk, &key);
    }
    writer.add_document(doc).map_err(StateError::tantivy)?;

    writer.commit().map_err(StateError::tantivy)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_target_resolves_expected_paths() {
        let dir = tempdir().unwrap();
        let data_dir = dir.path();

        let base = WriteTarget::Base.index_root(data_dir);
        assert_eq!(base, data_dir.join("base"));

        let overlay = WriteTarget::Overlay {
            branch: "feat/auth#2",
        }
        .index_root(data_dir);
        assert_eq!(overlay, data_dir.join("overlay").join("feat-auth%232"));

        let staging = WriteTarget::Staging { sync_id: "sync-1" }.index_root(data_dir);
        assert_eq!(staging, data_dir.join("staging").join("sync-1"));
    }

    #[test]
    fn open_index_set_for_target_initializes_overlay_and_staging() {
        let dir = tempdir().unwrap();
        let data_dir = dir.path();

        let _overlay = open_index_set_for_target(
            data_dir,
            WriteTarget::Overlay {
                branch: "feat/auth",
            },
        )
        .unwrap();
        let overlay_root = WriteTarget::Overlay {
            branch: "feat/auth",
        }
        .index_root(data_dir);
        assert!(overlay_root.join("symbols").exists());
        assert!(overlay_root.join("snippets").exists());
        assert!(overlay_root.join("files").exists());

        let _staging =
            open_index_set_for_target(data_dir, WriteTarget::Staging { sync_id: "sync-42" })
                .unwrap();
        let staging_root = WriteTarget::Staging { sync_id: "sync-42" }.index_root(data_dir);
        assert!(staging_root.join("symbols").exists());
        assert!(staging_root.join("snippets").exists());
        assert!(staging_root.join("files").exists());

        let _existing = open_existing_index_set_for_target(
            data_dir,
            WriteTarget::Staging { sync_id: "sync-42" },
        )
        .unwrap();
    }
}
