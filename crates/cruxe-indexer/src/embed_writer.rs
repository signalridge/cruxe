use cruxe_core::config::SemanticConfig;
use cruxe_core::error::StateError;
use cruxe_core::types::{SnippetRecord, SymbolRecord};
use cruxe_state::embedding::{self, EmbeddingProvider};
use cruxe_state::vector_index::{self, VectorRecord};
use rusqlite::Connection;

pub struct EmbeddingWriter {
    enabled: bool,
    project_id: String,
    ref_name: String,
    provider: Option<Box<dyn EmbeddingProvider + Send>>,
    external_provider_blocked: bool,
    vector_backend: Option<String>,
}

impl EmbeddingWriter {
    pub fn new(
        semantic: &SemanticConfig,
        project_id: &str,
        ref_name: &str,
    ) -> Result<Self, StateError> {
        let hybrid_enabled = semantic.mode.eq_ignore_ascii_case("hybrid");
        if !hybrid_enabled {
            return Ok(Self {
                enabled: false,
                project_id: project_id.to_string(),
                ref_name: ref_name.to_string(),
                provider: None,
                external_provider_blocked: false,
                vector_backend: None,
            });
        }

        let built = embedding::build_embedding_provider(semantic)?;
        Ok(Self {
            enabled: true,
            project_id: project_id.to_string(),
            ref_name: ref_name.to_string(),
            provider: Some(built.provider),
            external_provider_blocked: built.external_provider_blocked,
            vector_backend: semantic.vector_backend_opt().map(String::from),
        })
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn external_provider_blocked(&self) -> bool {
        self.external_provider_blocked
    }

    pub fn delete_for_ref(&self, conn: &Connection) -> Result<usize, StateError> {
        if !self.enabled {
            return Ok(0);
        }
        vector_index::delete_vectors_for_ref_with_backend(
            conn,
            &self.project_id,
            &self.ref_name,
            self.vector_backend.as_deref(),
        )
    }

    pub fn delete_for_path(&self, conn: &Connection, path: &str) -> Result<usize, StateError> {
        if !self.enabled {
            return Ok(0);
        }
        vector_index::delete_vectors_for_path_with_backend(
            conn,
            &self.project_id,
            &self.ref_name,
            path,
            self.vector_backend.as_deref(),
        )
    }

    pub fn delete_for_symbols(
        &self,
        conn: &Connection,
        symbol_stable_ids: &[String],
    ) -> Result<usize, StateError> {
        if !self.enabled || symbol_stable_ids.is_empty() {
            return Ok(0);
        }
        vector_index::delete_vectors_for_symbols_with_backend(
            conn,
            &self.project_id,
            &self.ref_name,
            symbol_stable_ids,
            self.vector_backend.as_deref(),
        )
    }

    pub fn delete_for_file_vectors(
        &self,
        conn: &Connection,
        path: &str,
    ) -> Result<usize, StateError> {
        let symbol_stable_ids: Vec<String> = cruxe_state::symbols::list_symbols_in_file(
            conn,
            &self.project_id,
            &self.ref_name,
            path,
        )?
        .into_iter()
        .map(|symbol| symbol.symbol_stable_id)
        .collect();
        self.delete_for_file_vectors_with_symbols(conn, path, &symbol_stable_ids)
    }

    pub fn delete_for_file_vectors_with_symbols(
        &self,
        conn: &Connection,
        path: &str,
        symbol_stable_ids: &[String],
    ) -> Result<usize, StateError> {
        let deleted_symbols = self.delete_for_symbols(conn, symbol_stable_ids)?;
        let deleted_path = self.delete_for_path(conn, path)?;
        Ok(deleted_symbols + deleted_path)
    }

    pub fn write_file_embeddings(
        &mut self,
        conn: &Connection,
        symbols: &[SymbolRecord],
        snippets: &[SnippetRecord],
    ) -> Result<usize, StateError> {
        self.write_embeddings_for_files(conn, std::iter::once((symbols, snippets)))
    }

    pub fn write_embeddings_for_files<'a, I>(
        &mut self,
        conn: &Connection,
        file_batches: I,
    ) -> Result<usize, StateError>
    where
        I: IntoIterator<Item = (&'a [SymbolRecord], &'a [SnippetRecord])>,
    {
        if !self.enabled {
            return Ok(0);
        }

        let Some(provider) = self.provider.as_mut() else {
            return Ok(0);
        };

        let mut candidates = Vec::new();
        for (symbols, snippets) in file_batches {
            if snippets.is_empty() {
                continue;
            }
            candidates.extend(build_embedding_candidates(symbols, snippets));
        }
        if candidates.is_empty() {
            return Ok(0);
        }

        let inputs: Vec<String> = candidates
            .iter()
            .map(|candidate| candidate.snippet_text.clone())
            .collect();
        let vectors = provider.embed_batch(&inputs)?;
        if vectors.len() != candidates.len() {
            return Err(StateError::external(format!(
                "embedding output size mismatch: expected={} got={}",
                candidates.len(),
                vectors.len()
            )));
        }

        let model_id = provider.model_id().to_string();
        let model_version = provider.model_version().to_string();
        let dimensions = provider.dimensions();

        let records: Vec<VectorRecord> = candidates
            .into_iter()
            .zip(vectors)
            .map(|(candidate, vector)| VectorRecord {
                project_id: self.project_id.clone(),
                ref_name: self.ref_name.clone(),
                symbol_stable_id: candidate.symbol_stable_id,
                snippet_hash: candidate.snippet_hash,
                embedding_model_id: model_id.clone(),
                embedding_model_version: model_version.clone(),
                embedding_dimensions: dimensions,
                path: candidate.path,
                line_start: candidate.line_start,
                line_end: candidate.line_end,
                language: candidate.language,
                chunk_type: candidate.chunk_type,
                snippet_text: candidate.snippet_text,
                vector,
            })
            .collect();

        vector_index::upsert_vectors_with_backend(conn, &records, self.vector_backend.as_deref())
    }
}

#[derive(Debug)]
struct EmbeddingSnippet {
    symbol_stable_id: String,
    snippet_hash: String,
    path: String,
    line_start: u32,
    line_end: u32,
    language: String,
    chunk_type: Option<String>,
    snippet_text: String,
}

fn build_embedding_candidates(
    symbols: &[SymbolRecord],
    snippets: &[SnippetRecord],
) -> Vec<EmbeddingSnippet> {
    snippets
        .iter()
        .filter_map(|snippet| {
            let symbol = best_symbol_for_snippet(symbols, snippet)?;
            let snippet_hash = blake3::hash(
                format!(
                    "{}|{}|{}|{}",
                    snippet.path, snippet.line_start, snippet.line_end, snippet.content
                )
                .as_bytes(),
            )
            .to_hex()
            .to_string();
            Some(EmbeddingSnippet {
                symbol_stable_id: symbol.symbol_stable_id.clone(),
                snippet_hash,
                path: snippet.path.clone(),
                line_start: snippet.line_start,
                line_end: snippet.line_end,
                language: snippet.language.clone(),
                chunk_type: Some(snippet.chunk_type.clone()),
                snippet_text: snippet.content.clone(),
            })
        })
        .collect()
}

fn best_symbol_for_snippet<'a>(
    symbols: &'a [SymbolRecord],
    snippet: &SnippetRecord,
) -> Option<&'a SymbolRecord> {
    symbols
        .iter()
        .filter(|symbol| {
            symbol.path == snippet.path
                && symbol.line_start <= snippet.line_start
                && symbol.line_end >= snippet.line_end
        })
        .min_by_key(|symbol| {
            let span = symbol.line_end.saturating_sub(symbol.line_start);
            let boundary_distance = symbol.line_start.abs_diff(snippet.line_start)
                + symbol.line_end.abs_diff(snippet.line_end);
            (span, boundary_distance)
        })
        .or_else(|| {
            symbols
                .iter()
                .filter(|symbol| symbol.path == snippet.path)
                .min_by_key(|symbol| {
                    symbol.line_start.abs_diff(snippet.line_start)
                        + symbol.line_end.abs_diff(snippet.line_end)
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::constants;
    use cruxe_core::types::{SnippetRecord, SymbolKind, SymbolRecord};
    use cruxe_state::{db, schema};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn setup_conn() -> Connection {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    fn hybrid_semantic_config() -> SemanticConfig {
        SemanticConfig {
            mode: "hybrid".to_string(),
            embedding: cruxe_core::config::SemanticEmbeddingConfig {
                provider: "local".to_string(),
                profile: "fast_local".to_string(),
                model: "NomicEmbedTextV15Q".to_string(),
                model_version: "fastembed-1".to_string(),
                dimensions: 768,
                batch_size: 8,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn fixture_repo_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/rust-sample")
            .canonicalize()
            .expect("fixture repo must exist at testdata/fixtures/rust-sample")
    }

    #[test]
    fn hybrid_writer_persists_vectors_with_metadata() {
        let conn = setup_conn();
        let mut writer = EmbeddingWriter::new(&hybrid_semantic_config(), "proj", "main").unwrap();

        let symbols = vec![SymbolRecord {
            repo: "proj".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/lib.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: "sym-1".to_string(),
            symbol_stable_id: "stable-1".to_string(),
            name: "auth".to_string(),
            qualified_name: "auth".to_string(),
            kind: SymbolKind::Function,
            signature: Some("fn auth()".to_string()),
            line_start: 10,
            line_end: 20,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: Some("fn auth() {}".to_string()),
        }];
        let snippets = vec![SnippetRecord {
            repo: "proj".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/lib.rs".to_string(),
            language: "rust".to_string(),
            chunk_type: "function_body".to_string(),
            imports: None,
            line_start: 10,
            line_end: 20,
            content: "fn auth() {}".to_string(),
        }];

        let written = writer
            .write_file_embeddings(&conn, &symbols, &snippets)
            .unwrap();
        assert_eq!(written, 1);

        let row: (String, String, String) = conn
            .query_row(
                "SELECT symbol_stable_id, embedding_model_version, path FROM semantic_vectors
                 WHERE project_id = 'proj' AND \"ref\" = 'main'
                 LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row.0, "stable-1");
        assert_eq!(row.1, "fastembed-1");
        assert_eq!(row.2, "src/lib.rs");
    }

    #[test]
    fn non_hybrid_mode_skips_embedding_writes() {
        let conn = setup_conn();
        let semantic = SemanticConfig::default();
        let mut writer = EmbeddingWriter::new(&semantic, "proj", "main").unwrap();
        assert!(!writer.enabled());

        let written = writer.write_file_embeddings(&conn, &[], &[]).unwrap();
        assert_eq!(written, 0);
    }

    #[test]
    fn delete_for_file_vectors_cleans_existing_file_embeddings() {
        let conn = setup_conn();
        let mut writer = EmbeddingWriter::new(&hybrid_semantic_config(), "proj", "main").unwrap();
        let symbol = SymbolRecord {
            repo: "proj".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/lib.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: "sym-1".to_string(),
            symbol_stable_id: "stable-1".to_string(),
            name: "auth".to_string(),
            qualified_name: "auth".to_string(),
            kind: SymbolKind::Function,
            signature: Some("fn auth()".to_string()),
            line_start: 10,
            line_end: 20,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: Some("fn auth() {}".to_string()),
        };
        let snippet = SnippetRecord {
            repo: "proj".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/lib.rs".to_string(),
            language: "rust".to_string(),
            chunk_type: "function_body".to_string(),
            imports: None,
            line_start: 10,
            line_end: 20,
            content: "fn auth() {}".to_string(),
        };

        cruxe_state::symbols::insert_symbol(&conn, &symbol).unwrap();
        writer
            .write_file_embeddings(&conn, &[symbol], &[snippet])
            .unwrap();

        let before_delete: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM semantic_vectors WHERE project_id = 'proj' AND \"ref\" = 'main'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before_delete, 1);

        let deleted = writer.delete_for_file_vectors(&conn, "src/lib.rs").unwrap();
        assert!(deleted >= 1);

        let after_delete: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM semantic_vectors WHERE project_id = 'proj' AND \"ref\" = 'main'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after_delete, 0);
    }

    #[test]
    fn write_embeddings_for_files_batches_multiple_inputs() {
        let conn = setup_conn();
        let mut writer = EmbeddingWriter::new(&hybrid_semantic_config(), "proj", "main").unwrap();

        let symbols_file_a = vec![SymbolRecord {
            repo: "proj".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/a.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: "sym-a".to_string(),
            symbol_stable_id: "stable-a".to_string(),
            name: "a".to_string(),
            qualified_name: "a".to_string(),
            kind: SymbolKind::Function,
            signature: Some("fn a()".to_string()),
            line_start: 1,
            line_end: 5,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: Some("fn a() {}".to_string()),
        }];
        let snippets_file_a = vec![SnippetRecord {
            repo: "proj".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/a.rs".to_string(),
            language: "rust".to_string(),
            chunk_type: "function_body".to_string(),
            imports: None,
            line_start: 1,
            line_end: 5,
            content: "fn a() {}".to_string(),
        }];

        let symbols_file_b = vec![SymbolRecord {
            repo: "proj".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/b.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: "sym-b".to_string(),
            symbol_stable_id: "stable-b".to_string(),
            name: "b".to_string(),
            qualified_name: "b".to_string(),
            kind: SymbolKind::Function,
            signature: Some("fn b()".to_string()),
            line_start: 10,
            line_end: 16,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: Some("fn b() {}".to_string()),
        }];
        let snippets_file_b = vec![SnippetRecord {
            repo: "proj".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/b.rs".to_string(),
            language: "rust".to_string(),
            chunk_type: "function_body".to_string(),
            imports: None,
            line_start: 10,
            line_end: 16,
            content: "fn b() {}".to_string(),
        }];

        let written = writer
            .write_embeddings_for_files(
                &conn,
                [
                    (symbols_file_a.as_slice(), snippets_file_a.as_slice()),
                    (symbols_file_b.as_slice(), snippets_file_b.as_slice()),
                ],
            )
            .unwrap();
        assert_eq!(written, 2);
    }

    #[test]
    fn hybrid_writer_indexes_fixture_repo_with_complete_vector_metadata() {
        let conn = setup_conn();
        let mut writer = EmbeddingWriter::new(&hybrid_semantic_config(), "proj", "main").unwrap();
        let repo_root = fixture_repo_path();
        let files = crate::scanner::scan_directory(&repo_root, constants::MAX_FILE_SIZE);

        let mut expected_vectors = 0usize;
        for file in files {
            let content = std::fs::read_to_string(&file.path).expect("read fixture file");
            if !crate::parser::is_language_supported(&file.language) {
                continue;
            }
            let tree = match crate::parser::parse_file(&content, &file.language) {
                Ok(tree) => tree,
                Err(_) => continue,
            };
            let extracted = crate::languages::extract_symbols(&tree, &content, &file.language);
            let symbols = crate::symbol_extract::build_symbol_records(
                &extracted,
                "proj",
                "main",
                &file.relative_path,
                None,
            );
            let snippets = crate::snippet_extract::build_snippet_records(
                &extracted,
                "proj",
                "main",
                &file.relative_path,
                None,
            );

            let file_candidates = build_embedding_candidates(&symbols, &snippets).len();
            expected_vectors += file_candidates;
            let written = writer
                .write_file_embeddings(&conn, &symbols, &snippets)
                .expect("write fixture embeddings");
            assert_eq!(
                written, file_candidates,
                "written embedding count should match candidate snippets for {}",
                file.relative_path
            );
        }

        let stored_vectors: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM semantic_vectors
                 WHERE project_id = 'proj' AND \"ref\" = 'main' AND embedding_model_version = 'fastembed-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_vectors, expected_vectors);

        let invalid_metadata_rows: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM semantic_vectors
                 WHERE project_id = 'proj' AND \"ref\" = 'main'
                   AND (
                     symbol_stable_id = ''
                     OR snippet_hash = ''
                     OR embedding_model_version != 'fastembed-1'
                     OR embedding_dimensions != 768
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(invalid_metadata_rows, 0);
    }
}
