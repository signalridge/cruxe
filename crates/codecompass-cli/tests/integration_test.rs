//! Integration tests for CodeCompass.
//!
//! T031: Init + Doctor Roundtrip
//! T043: Index + Locate Symbol
//! T054: Search Error Intent
//! T055: Search Path Intent
//! T056: Search Snippet Join Enrichment
//! T071: Ref-Scoped Search Isolation

use std::path::{Path, PathBuf};
use tempfile::tempdir;

use codecompass_core::types::QueryIntent;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the absolute path to the fixture repo used in T043.
fn fixture_repo_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .join("../../testdata/fixtures/rust-sample")
        .canonicalize()
        .expect("fixture repo must exist at testdata/fixtures/rust-sample")
}

/// Perform the "init" lifecycle at a library level:
///   1. Create the data directory under `data_root`.
///   2. Open SQLite and create schema.
///   3. Register the project.
///   4. Open Tantivy indices.
///
/// Returns `(project_id, data_dir)` for downstream assertions.
fn do_init(repo_root: &Path, data_root: &Path) -> (String, PathBuf) {
    let repo_root_str = repo_root.to_string_lossy().to_string();
    let project_id = codecompass_core::types::generate_project_id(&repo_root_str);
    let data_dir = data_root.join("data").join(&project_id);
    std::fs::create_dir_all(&data_dir).expect("create data dir");

    // SQLite
    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).expect("open sqlite");
    codecompass_state::schema::create_tables(&conn).expect("create tables");

    // Detect VCS mode
    let vcs_mode = repo_root.join(".git").exists();
    let default_ref = if vcs_mode {
        "main".to_string()
    } else {
        codecompass_core::constants::REF_LIVE.to_string()
    };

    let now = "2026-01-01T00:00:00Z".to_string();
    let project = codecompass_core::types::Project {
        project_id: project_id.clone(),
        repo_root: repo_root_str.clone(),
        display_name: repo_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string()),
        default_ref,
        vcs_mode,
        schema_version: codecompass_core::constants::SCHEMA_VERSION,
        parser_version: codecompass_core::constants::PARSER_VERSION,
        created_at: now.clone(),
        updated_at: now,
    };
    codecompass_state::project::create_project(&conn, &project).expect("create project");

    // Tantivy indices
    let _index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy indices");

    (project_id, data_dir)
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create destination directory");
    for entry in std::fs::read_dir(src).expect("read source directory") {
        let entry = entry.expect("read directory entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry
            .file_type()
            .expect("read file type for source entry")
            .is_dir()
        {
            copy_dir_recursive(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap_or_else(|e| {
                panic!(
                    "copy file '{}' -> '{}' failed: {}",
                    from.display(),
                    to.display(),
                    e
                )
            });
        }
    }
}

fn index_repo_with_import_edges(
    repo_root: &Path,
    project_id: &str,
    effective_ref: &str,
    index_set: &codecompass_state::tantivy_index::IndexSet,
    conn: &rusqlite::Connection,
) {
    let files = codecompass_indexer::scanner::scan_directory(
        repo_root,
        codecompass_core::constants::MAX_FILE_SIZE,
    );
    let mut pending_imports = Vec::new();

    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read fixture file");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let (extracted, raw_imports) =
            if codecompass_indexer::parser::is_language_supported(&file.language) {
                match codecompass_indexer::parser::parse_file(&content, &file.language) {
                    Ok(tree) => (
                        codecompass_indexer::languages::extract_symbols(
                            &tree,
                            &content,
                            &file.language,
                        ),
                        codecompass_indexer::import_extract::extract_imports(
                            &tree,
                            &content,
                            &file.language,
                            &file.relative_path,
                        ),
                    ),
                    Err(_) => (Vec::new(), Vec::new()),
                }
            } else {
                (Vec::new(), Vec::new())
            };

        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.to_string(),
            r#ref: effective_ref.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: Some(content.lines().take(20).collect::<Vec<_>>().join("\n")),
        };

        codecompass_state::symbols::delete_symbols_for_file(
            conn,
            project_id,
            effective_ref,
            &file.relative_path,
        )
        .expect("delete symbols for file");
        codecompass_indexer::writer::write_file_records(
            index_set,
            conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .expect("write indexed file records");
        pending_imports.push((file.relative_path.clone(), raw_imports));
    }

    for (path, raw_imports) in pending_imports {
        codecompass_indexer::writer::replace_import_edges_for_file(
            conn,
            project_id,
            effective_ref,
            &path,
            raw_imports,
        )
        .expect("replace import edges for file");
    }
}

// ===========================================================================
// T031 -- Init + Doctor Roundtrip
// ===========================================================================

#[test]
fn t031_init_creates_sqlite_tables() {
    let tmp = tempdir().expect("tempdir");
    let repo_root = tmp.path().join("my-project");
    std::fs::create_dir_all(&repo_root).unwrap();
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    let (project_id, data_dir) = do_init(&repo_root, &data_root);

    // Verify SQLite tables exist
    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let expected = [
        "branch_state",
        "branch_tombstones",
        "file_manifest",
        "index_jobs",
        "known_workspaces",
        "projects",
        "symbol_edges",
        "symbol_relations",
    ];
    for t in &expected {
        assert!(
            tables.contains(&t.to_string()),
            "expected table '{}' to exist, found: {:?}",
            t,
            tables
        );
    }

    // Verify project registration
    let repo_root_str = repo_root.to_string_lossy().to_string();
    let proj = codecompass_state::project::get_by_root(&conn, &repo_root_str)
        .expect("query project")
        .expect("project should exist");
    assert_eq!(proj.project_id, project_id);
}

#[test]
fn t031_init_creates_tantivy_indices() {
    let tmp = tempdir().expect("tempdir");
    let repo_root = tmp.path().join("my-project");
    std::fs::create_dir_all(&repo_root).unwrap();
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    let (_project_id, data_dir) = do_init(&repo_root, &data_root);

    // Verify the three Tantivy index directories exist and can be opened
    let base_dir = data_dir.join("base");
    for index_name in &["symbols", "snippets", "files"] {
        let index_dir = base_dir.join(index_name);
        assert!(
            index_dir.exists(),
            "Tantivy index dir '{}' must exist",
            index_name
        );
        tantivy::Index::open_in_dir(&index_dir)
            .unwrap_or_else(|e| panic!("should open Tantivy index '{}': {}", index_name, e));
    }
}

#[test]
fn t031_doctor_runs_after_init() {
    let tmp = tempdir().expect("tempdir");
    let repo_root = tmp.path().join("my-project");
    std::fs::create_dir_all(&repo_root).unwrap();
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    let (project_id, data_dir) = do_init(&repo_root, &data_root);

    // ---- replicate doctor checks at library level ----

    // 1. SQLite integrity check
    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("integrity_check query");
    assert_eq!(integrity, "ok", "SQLite integrity check must pass");

    // 2. Project registration lookup
    let repo_root_str = repo_root.to_string_lossy().to_string();
    let proj = codecompass_state::project::get_by_root(&conn, &repo_root_str)
        .expect("query project")
        .expect("project should exist after init");
    assert_eq!(proj.project_id, project_id);

    // 3. Tantivy indices open successfully
    let base_dir = data_dir.join("base");
    for index_name in &["symbols", "snippets", "files"] {
        let index_dir = base_dir.join(index_name);
        tantivy::Index::open_in_dir(&index_dir)
            .unwrap_or_else(|e| panic!("doctor: Tantivy '{}' should open: {}", index_name, e));
    }

    // 4. Tree-sitter grammars (doctor checks these)
    for lang in codecompass_indexer::parser::supported_languages() {
        assert!(
            codecompass_indexer::parser::is_language_supported(lang),
            "language '{}' should be supported",
            lang
        );
        codecompass_indexer::parser::get_language(lang)
            .unwrap_or_else(|e| panic!("get_language('{}') failed: {}", lang, e));
    }
}

#[test]
fn t031_init_idempotent() {
    // Running init twice should not fail; the second call creates the
    // project again only if it does not exist (we test the building blocks).
    let tmp = tempdir().expect("tempdir");
    let repo_root = tmp.path().join("my-project");
    std::fs::create_dir_all(&repo_root).unwrap();
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    let (project_id, data_dir) = do_init(&repo_root, &data_root);

    // A second call to create_tables is idempotent (IF NOT EXISTS).
    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    codecompass_state::schema::create_tables(&conn).expect("idempotent create_tables");

    // Re-opening Tantivy indices is also idempotent.
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("reopen tantivy");
    assert!(index_set.symbols.schema().get_field("symbol_exact").is_ok());

    // The project should still be there.
    let repo_root_str = repo_root.to_string_lossy().to_string();
    let proj = codecompass_state::project::get_by_root(&conn, &repo_root_str)
        .expect("query")
        .expect("project exists");
    assert_eq!(proj.project_id, project_id);
}

// ===========================================================================
// T043 -- Index + Locate Symbol
// ===========================================================================

/// Full index pipeline at library level:
///   scan -> parse -> extract -> write -> locate
#[test]
fn t043_index_and_locate_validate_token() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    // -- Init phase --
    let (project_id, data_dir) = do_init(&fixture, &data_root);

    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");

    // The fixture has no .git, so default_ref is "live"
    let effective_ref = codecompass_core::constants::REF_LIVE;

    // -- Index phase: scan, parse, extract, write --
    let files = codecompass_indexer::scanner::scan_directory(
        &fixture,
        codecompass_core::constants::MAX_FILE_SIZE,
    );
    assert!(
        !files.is_empty(),
        "scanner should discover .rs files in the fixture"
    );

    let mut total_symbols: u64 = 0;
    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read fixture file");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        // Parse
        let extracted = if codecompass_indexer::parser::is_language_supported(&file.language) {
            match codecompass_indexer::parser::parse_file(&content, &file.language) {
                Ok(tree) => {
                    codecompass_indexer::languages::extract_symbols(&tree, &content, &file.language)
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        // Build records
        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );

        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.clone(),
            r#ref: effective_ref.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: Some(content.lines().take(20).collect::<Vec<_>>().join("\n")),
        };

        // Delete old records (idempotent on first run)
        codecompass_state::symbols::delete_symbols_for_file(
            &conn,
            &project_id,
            effective_ref,
            &file.relative_path,
        )
        .unwrap();

        // Write
        total_symbols += symbols.len() as u64;
        codecompass_indexer::writer::write_file_records(
            &index_set,
            &conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .expect("write_file_records");
    }

    assert!(
        total_symbols > 0,
        "should have extracted at least one symbol from the fixture"
    );

    // -- Locate phase --
    let results = codecompass_query::locate::locate_symbol(
        &index_set.symbols,
        "validate_token",
        None,
        None,
        None,
        10,
    )
    .expect("locate_symbol should succeed");

    assert!(
        !results.is_empty(),
        "locate_symbol('validate_token') must return at least one result"
    );

    let first = &results[0];
    assert_eq!(first.name, "validate_token");
    assert!(
        first.path.contains("auth.rs"),
        "validate_token should be in auth.rs, got path='{}'",
        first.path
    );
    assert_eq!(first.kind, "function", "validate_token is a function");
    assert_eq!(first.language, "rust");
}

#[test]
fn t043_index_and_locate_with_kind_filter() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    let (project_id, data_dir) = do_init(&fixture, &data_root);
    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");
    let effective_ref = codecompass_core::constants::REF_LIVE;

    // Index all files
    let files = codecompass_indexer::scanner::scan_directory(
        &fixture,
        codecompass_core::constants::MAX_FILE_SIZE,
    );
    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let extracted = if codecompass_indexer::parser::is_language_supported(&file.language) {
            match codecompass_indexer::parser::parse_file(&content, &file.language) {
                Ok(tree) => {
                    codecompass_indexer::languages::extract_symbols(&tree, &content, &file.language)
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.clone(),
            r#ref: effective_ref.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: None,
        };

        codecompass_state::symbols::delete_symbols_for_file(
            &conn,
            &project_id,
            effective_ref,
            &file.relative_path,
        )
        .unwrap();
        codecompass_indexer::writer::write_file_records(
            &index_set,
            &conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .unwrap();
    }

    // locate with kind=function should return validate_token
    let results = codecompass_query::locate::locate_symbol(
        &index_set.symbols,
        "validate_token",
        Some("function"),
        None,
        None,
        10,
    )
    .expect("locate with kind filter");
    assert!(
        !results.is_empty(),
        "should find validate_token as function"
    );

    // locate with kind=struct should NOT return validate_token
    let results_struct = codecompass_query::locate::locate_symbol(
        &index_set.symbols,
        "validate_token",
        Some("struct"),
        None,
        None,
        10,
    )
    .expect("locate with struct filter");
    assert!(
        results_struct.is_empty(),
        "validate_token is not a struct, should return nothing"
    );
}

#[test]
fn t043_index_discovers_multiple_symbols() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    let (project_id, data_dir) = do_init(&fixture, &data_root);
    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");
    let effective_ref = codecompass_core::constants::REF_LIVE;

    // Index
    let files = codecompass_indexer::scanner::scan_directory(
        &fixture,
        codecompass_core::constants::MAX_FILE_SIZE,
    );
    let mut all_symbol_names: Vec<String> = Vec::new();

    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let extracted = if codecompass_indexer::parser::is_language_supported(&file.language) {
            match codecompass_indexer::parser::parse_file(&content, &file.language) {
                Ok(tree) => {
                    codecompass_indexer::languages::extract_symbols(&tree, &content, &file.language)
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.clone(),
            r#ref: effective_ref.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: None,
        };

        for sym in &symbols {
            all_symbol_names.push(sym.name.clone());
        }

        codecompass_state::symbols::delete_symbols_for_file(
            &conn,
            &project_id,
            effective_ref,
            &file.relative_path,
        )
        .unwrap();
        codecompass_indexer::writer::write_file_records(
            &index_set,
            &conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .unwrap();
    }

    // The fixture should contain several known symbols
    assert!(
        all_symbol_names.contains(&"validate_token".to_string()),
        "should find validate_token; found: {:?}",
        all_symbol_names
    );
    assert!(
        all_symbol_names.contains(&"require_role".to_string()),
        "should find require_role; found: {:?}",
        all_symbol_names
    );

    // Verify we can also locate require_role via Tantivy
    let results = codecompass_query::locate::locate_symbol(
        &index_set.symbols,
        "require_role",
        None,
        None,
        None,
        10,
    )
    .expect("locate require_role");
    assert!(
        !results.is_empty(),
        "should locate require_role in Tantivy index"
    );
    assert!(
        results[0].path.contains("auth.rs"),
        "require_role should be in auth.rs"
    );

    // Verify SQLite symbol count is positive
    let count =
        codecompass_state::symbols::symbol_count(&conn, &project_id, effective_ref).unwrap();
    assert!(
        count > 0,
        "SQLite symbol_relations should have records after indexing"
    );

    // Verify file manifest was populated
    let file_count =
        codecompass_state::manifest::file_count(&conn, &project_id, effective_ref).unwrap();
    assert!(
        file_count > 0,
        "file_manifest should have records after indexing"
    );
}

#[test]
fn t043_locate_nonexistent_symbol_returns_empty() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    let (project_id, data_dir) = do_init(&fixture, &data_root);
    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");
    let effective_ref = codecompass_core::constants::REF_LIVE;

    // Index the fixture
    let files = codecompass_indexer::scanner::scan_directory(
        &fixture,
        codecompass_core::constants::MAX_FILE_SIZE,
    );
    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let extracted = if codecompass_indexer::parser::is_language_supported(&file.language) {
            match codecompass_indexer::parser::parse_file(&content, &file.language) {
                Ok(tree) => {
                    codecompass_indexer::languages::extract_symbols(&tree, &content, &file.language)
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.clone(),
            r#ref: effective_ref.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: None,
        };

        codecompass_state::symbols::delete_symbols_for_file(
            &conn,
            &project_id,
            effective_ref,
            &file.relative_path,
        )
        .unwrap();
        codecompass_indexer::writer::write_file_records(
            &index_set,
            &conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .unwrap();
    }

    // Searching for a symbol that does not exist should return an empty vec
    let results = codecompass_query::locate::locate_symbol(
        &index_set.symbols,
        "this_symbol_does_not_exist_anywhere",
        None,
        None,
        None,
        10,
    )
    .expect("locate nonexistent");
    assert!(
        results.is_empty(),
        "nonexistent symbol should return empty results"
    );
}

// ===========================================================================
// T054 -- Search: Error Intent
// ===========================================================================

/// Verify that `search_code` classifies an error-like query as `QueryIntent::Error`
/// and returns a valid `SearchResponse`.
#[test]
fn t054_search_error_intent() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    // -- Init phase --
    let (project_id, data_dir) = do_init(&fixture, &data_root);

    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");
    let effective_ref = codecompass_core::constants::REF_LIVE;

    // -- Index phase: scan, parse, extract, write --
    let files = codecompass_indexer::scanner::scan_directory(
        &fixture,
        codecompass_core::constants::MAX_FILE_SIZE,
    );
    assert!(
        !files.is_empty(),
        "scanner should discover .rs files in the fixture"
    );

    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read fixture file");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let extracted = if codecompass_indexer::parser::is_language_supported(&file.language) {
            match codecompass_indexer::parser::parse_file(&content, &file.language) {
                Ok(tree) => {
                    codecompass_indexer::languages::extract_symbols(&tree, &content, &file.language)
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.clone(),
            r#ref: effective_ref.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: Some(content.lines().take(20).collect::<Vec<_>>().join("\n")),
        };

        codecompass_state::symbols::delete_symbols_for_file(
            &conn,
            &project_id,
            effective_ref,
            &file.relative_path,
        )
        .unwrap();
        codecompass_indexer::writer::write_file_records(
            &index_set,
            &conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .expect("write_file_records");
    }

    // -- Search phase --
    // The query "exception handler" contains the word "exception" which
    // triggers the Error intent in the classifier.
    let response = codecompass_query::search::search_code(
        &index_set,
        Some(&conn),
        "exception handler",
        None,
        None,
        10,
        false,
    )
    .expect("search_code should succeed");

    assert_eq!(
        response.query_intent,
        QueryIntent::Error,
        "query containing 'exception' should be classified as Error intent, got {:?}",
        response.query_intent,
    );
}

// ===========================================================================
// T055 -- Search: Path Intent
// ===========================================================================

/// Verify that `search_code` classifies a path-like query as `QueryIntent::Path`
/// and returns a valid `SearchResponse`.
#[test]
fn t055_search_path_intent() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    // -- Init phase --
    let (project_id, data_dir) = do_init(&fixture, &data_root);

    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");
    let effective_ref = codecompass_core::constants::REF_LIVE;

    // -- Index phase: scan, parse, extract, write --
    let files = codecompass_indexer::scanner::scan_directory(
        &fixture,
        codecompass_core::constants::MAX_FILE_SIZE,
    );
    assert!(
        !files.is_empty(),
        "scanner should discover .rs files in the fixture"
    );

    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read fixture file");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let extracted = if codecompass_indexer::parser::is_language_supported(&file.language) {
            match codecompass_indexer::parser::parse_file(&content, &file.language) {
                Ok(tree) => {
                    codecompass_indexer::languages::extract_symbols(&tree, &content, &file.language)
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.clone(),
            r#ref: effective_ref.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: Some(content.lines().take(20).collect::<Vec<_>>().join("\n")),
        };

        codecompass_state::symbols::delete_symbols_for_file(
            &conn,
            &project_id,
            effective_ref,
            &file.relative_path,
        )
        .unwrap();
        codecompass_indexer::writer::write_file_records(
            &index_set,
            &conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .expect("write_file_records");
    }

    // -- Search phase --
    // The query "src/auth/handler.rs" contains a '/' path separator,
    // which triggers the Path intent in the classifier.
    let response = codecompass_query::search::search_code(
        &index_set,
        Some(&conn),
        "src/auth/handler.rs",
        None,
        None,
        10,
        false,
    )
    .expect("search_code should succeed");

    assert_eq!(
        response.query_intent,
        QueryIntent::Path,
        "query with path separators should be classified as Path intent, got {:?}",
        response.query_intent,
    );

    // If any results were returned, verify they have file metadata populated
    for result in &response.results {
        assert!(
            !result.path.is_empty(),
            "search result should have a non-empty path"
        );
        assert!(
            !result.language.is_empty(),
            "search result should have a non-empty language"
        );
    }
}

// ===========================================================================
// T056 -- Search Snippet Join Enrichment
// ===========================================================================

/// Verify that snippet results are enriched with symbol metadata via SQLite join.
#[test]
fn t056_search_snippet_results_include_symbol_metadata_when_available() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    // -- Init phase --
    let (project_id, data_dir) = do_init(&fixture, &data_root);

    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");
    let effective_ref = codecompass_core::constants::REF_LIVE;

    // -- Index phase: scan, parse, extract, write --
    let files = codecompass_indexer::scanner::scan_directory(
        &fixture,
        codecompass_core::constants::MAX_FILE_SIZE,
    );
    assert!(
        !files.is_empty(),
        "scanner should discover .rs files in the fixture"
    );

    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read fixture file");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let extracted = if codecompass_indexer::parser::is_language_supported(&file.language) {
            match codecompass_indexer::parser::parse_file(&content, &file.language) {
                Ok(tree) => {
                    codecompass_indexer::languages::extract_symbols(&tree, &content, &file.language)
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.clone(),
            r#ref: effective_ref.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: Some(content.lines().take(20).collect::<Vec<_>>().join("\n")),
        };

        codecompass_state::symbols::delete_symbols_for_file(
            &conn,
            &project_id,
            effective_ref,
            &file.relative_path,
        )
        .unwrap();
        codecompass_indexer::writer::write_file_records(
            &index_set,
            &conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .expect("write_file_records");
    }

    let response = codecompass_query::search::search_code(
        &index_set,
        Some(&conn),
        "missing Bearer prefix",
        None,
        Some("rust"),
        50,
        false,
    )
    .expect("search_code should succeed");

    let enriched_snippet = response.results.iter().find(|r| {
        r.result_type == "snippet"
            && r.symbol_id.is_some()
            && r.symbol_stable_id.is_some()
            && r.kind.is_some()
            && r.name.is_some()
            && r.qualified_name.is_some()
    });

    assert!(
        enriched_snippet.is_some(),
        "expected at least one snippet result enriched with symbol metadata"
    );
}

// ===========================================================================
// T071 -- Ref-Scoped Search Isolation
// ===========================================================================

/// Verify that indexing under different refs produces isolated search results.
/// A symbol indexed only on "feat/auth" must NOT be visible when querying "main".
#[test]
fn t071_ref_scoped_search_isolates_branches() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    let (project_id, data_dir) = do_init(&fixture, &data_root);
    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");

    // --- Index all fixture files under ref="main" ---
    let ref_main = "main";
    let files = codecompass_indexer::scanner::scan_directory(
        &fixture,
        codecompass_core::constants::MAX_FILE_SIZE,
    );
    assert!(
        !files.is_empty(),
        "scanner should discover files in the fixture"
    );

    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read fixture file");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let extracted = if codecompass_indexer::parser::is_language_supported(&file.language) {
            match codecompass_indexer::parser::parse_file(&content, &file.language) {
                Ok(tree) => {
                    codecompass_indexer::languages::extract_symbols(&tree, &content, &file.language)
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            &project_id,
            ref_main,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            &project_id,
            ref_main,
            &file.relative_path,
            None,
        );
        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.clone(),
            r#ref: ref_main.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: None,
        };

        codecompass_state::symbols::delete_symbols_for_file(
            &conn,
            &project_id,
            ref_main,
            &file.relative_path,
        )
        .unwrap();
        codecompass_indexer::writer::write_file_records(
            &index_set,
            &conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .expect("write_file_records for main");
    }

    // --- Index the same fixture files under ref="feat/auth" plus an extra symbol ---
    let ref_feat = "feat/auth";

    // Index the shared fixture files under feat/auth
    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read fixture file");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let extracted = if codecompass_indexer::parser::is_language_supported(&file.language) {
            match codecompass_indexer::parser::parse_file(&content, &file.language) {
                Ok(tree) => {
                    codecompass_indexer::languages::extract_symbols(&tree, &content, &file.language)
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            &project_id,
            ref_feat,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            &project_id,
            ref_feat,
            &file.relative_path,
            None,
        );
        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.clone(),
            r#ref: ref_feat.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: None,
        };

        codecompass_state::symbols::delete_symbols_for_file(
            &conn,
            &project_id,
            ref_feat,
            &file.relative_path,
        )
        .unwrap();
        codecompass_indexer::writer::write_file_records(
            &index_set,
            &conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .expect("write_file_records for feat/auth shared files");
    }

    // Create a synthetic symbol that only exists on feat/auth
    let extra_symbol = codecompass_core::types::SymbolRecord {
        repo: project_id.clone(),
        r#ref: ref_feat.to_string(),
        commit: None,
        path: "src/new_feature.rs".to_string(),
        symbol_id: codecompass_core::types::compute_symbol_id(
            &project_id,
            ref_feat,
            "src/new_feature.rs",
            &codecompass_core::types::SymbolKind::Function,
            1,
            "branch_only_function",
        ),
        symbol_stable_id: codecompass_core::types::compute_symbol_stable_id(
            "rust",
            &codecompass_core::types::SymbolKind::Function,
            "crate::branch_only_function",
            None,
        ),
        name: "branch_only_function".to_string(),
        qualified_name: "crate::branch_only_function".to_string(),
        kind: codecompass_core::types::SymbolKind::Function,
        language: "rust".to_string(),
        line_start: 1,
        line_end: 5,
        signature: Some("fn branch_only_function()".to_string()),
        parent_symbol_id: None,
        visibility: Some("pub".to_string()),
        content: Some("fn branch_only_function() {}".to_string()),
    };

    let extra_file_record = codecompass_core::types::FileRecord {
        repo: project_id.clone(),
        r#ref: ref_feat.to_string(),
        commit: None,
        path: "src/new_feature.rs".to_string(),
        filename: "new_feature.rs".to_string(),
        language: "rust".to_string(),
        content_hash: blake3::hash(b"fn branch_only_function() {}")
            .to_hex()
            .to_string(),
        size_bytes: 28,
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        content_head: Some("fn branch_only_function() {}".to_string()),
    };

    // Write the extra symbol via the standard writer pipeline
    codecompass_indexer::writer::write_file_records(
        &index_set,
        &conn,
        &[extra_symbol],
        &[],
        &extra_file_record,
    )
    .expect("write_file_records for feat/auth extra symbol");

    // --- Locate with ref="main" -> should NOT find branch_only_function ---
    let results_main = codecompass_query::locate::locate_symbol(
        &index_set.symbols,
        "branch_only_function",
        None,
        None,
        Some("main"),
        10,
    )
    .expect("locate on main");
    assert!(
        results_main.is_empty(),
        "branch_only_function should NOT be visible on main, but got {} results",
        results_main.len()
    );

    // --- Locate with ref="feat/auth" -> should find it ---
    let results_feat = codecompass_query::locate::locate_symbol(
        &index_set.symbols,
        "branch_only_function",
        None,
        None,
        Some("feat/auth"),
        10,
    )
    .expect("locate on feat/auth");
    assert!(
        !results_feat.is_empty(),
        "branch_only_function SHOULD be visible on feat/auth"
    );
    assert_eq!(results_feat[0].name, "branch_only_function");

    // --- Locate without ref filter -> should find it (no isolation) ---
    let results_any = codecompass_query::locate::locate_symbol(
        &index_set.symbols,
        "branch_only_function",
        None,
        None,
        None,
        10,
    )
    .expect("locate without ref filter");
    assert!(
        !results_any.is_empty(),
        "branch_only_function should be visible with no ref filter"
    );

    // --- Shared symbol: validate_token should be on BOTH refs ---
    let vt_main = codecompass_query::locate::locate_symbol(
        &index_set.symbols,
        "validate_token",
        None,
        None,
        Some("main"),
        10,
    )
    .expect("locate validate_token on main");
    assert!(!vt_main.is_empty(), "validate_token should be on main");

    let vt_feat = codecompass_query::locate::locate_symbol(
        &index_set.symbols,
        "validate_token",
        None,
        None,
        Some("feat/auth"),
        10,
    )
    .expect("locate validate_token on feat/auth");
    assert!(!vt_feat.is_empty(), "validate_token should be on feat/auth");
}

// ===========================================================================
// T157/T158 -- Import edge indexing integration
// ===========================================================================

#[test]
fn t157_index_populates_import_edges_for_rust_fixture() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    let (project_id, data_dir) = do_init(&fixture, &data_root);
    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");
    let effective_ref = codecompass_core::constants::REF_LIVE;

    index_repo_with_import_edges(&fixture, &project_id, effective_ref, &index_set, &conn);

    let claims_symbol = codecompass_state::symbols::find_symbols_by_name(
        &conn,
        &project_id,
        effective_ref,
        "Claims",
        Some("src/auth.rs"),
    )
    .expect("query Claims symbol");
    assert_eq!(
        claims_symbol.len(),
        1,
        "Claims should be unique in src/auth.rs"
    );
    let claims_stable_id = claims_symbol[0].symbol_stable_id.clone();

    let source_symbol_id =
        codecompass_indexer::import_extract::source_symbol_id_for_path("src/handler.rs");
    let edges_from_handler = codecompass_state::edges::get_edges_from(
        &conn,
        &project_id,
        effective_ref,
        &source_symbol_id,
    )
    .expect("query import edges from handler");

    assert!(
        !edges_from_handler.is_empty(),
        "src/handler.rs should produce import edges"
    );
    assert!(
        edges_from_handler
            .iter()
            .any(|edge| edge.to_symbol_id == claims_stable_id),
        "expected import edge from src/handler.rs to Claims symbol"
    );
    assert!(
        edges_from_handler
            .iter()
            .all(|edge| edge.edge_type == "imports" && edge.confidence == "static"),
        "all extracted edges should be imports/static"
    );
}

#[test]
fn t158_reindex_replaces_import_edges_without_stale_rows() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    copy_dir_recursive(&fixture, &workspace);

    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();
    let (project_id, data_dir) = do_init(&workspace, &data_root);
    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");
    let effective_ref = codecompass_core::constants::REF_LIVE;

    index_repo_with_import_edges(&workspace, &project_id, effective_ref, &index_set, &conn);

    let connection_symbol = codecompass_state::symbols::find_symbols_by_name(
        &conn,
        &project_id,
        effective_ref,
        "Connection",
        Some("src/db.rs"),
    )
    .expect("query Connection symbol");
    assert_eq!(
        connection_symbol.len(),
        1,
        "Connection should be unique in src/db.rs"
    );
    let connection_stable_id = connection_symbol[0].symbol_stable_id.clone();

    let claims_symbol = codecompass_state::symbols::find_symbols_by_name(
        &conn,
        &project_id,
        effective_ref,
        "Claims",
        Some("src/auth.rs"),
    )
    .expect("query Claims symbol");
    assert_eq!(
        claims_symbol.len(),
        1,
        "Claims should be unique in src/auth.rs"
    );
    let claims_stable_id = claims_symbol[0].symbol_stable_id.clone();

    let source_symbol_id =
        codecompass_indexer::import_extract::source_symbol_id_for_path("src/lib.rs");
    let before_edges = codecompass_state::edges::get_edges_from(
        &conn,
        &project_id,
        effective_ref,
        &source_symbol_id,
    )
    .expect("query import edges before re-index");
    assert!(
        before_edges
            .iter()
            .any(|edge| edge.to_symbol_id == connection_stable_id),
        "precondition failed: src/lib.rs should import Connection before edit"
    );

    let lib_path = workspace.join("src/lib.rs");
    let original = std::fs::read_to_string(&lib_path).expect("read src/lib.rs");
    assert!(
        original.contains("use crate::db::Connection;"),
        "fixture precondition failed: src/lib.rs should import Connection"
    );
    let updated = original.replace("use crate::db::Connection;", "use crate::auth::Claims;");
    std::fs::write(&lib_path, updated).expect("write updated src/lib.rs");

    index_repo_with_import_edges(&workspace, &project_id, effective_ref, &index_set, &conn);

    let after_edges = codecompass_state::edges::get_edges_from(
        &conn,
        &project_id,
        effective_ref,
        &source_symbol_id,
    )
    .expect("query import edges after re-index");

    assert!(
        after_edges
            .iter()
            .all(|edge| edge.to_symbol_id != connection_stable_id),
        "stale Connection import edge should be removed after re-index"
    );
    assert!(
        after_edges
            .iter()
            .any(|edge| edge.to_symbol_id == claims_stable_id),
        "updated src/lib.rs import should point to Claims after re-index"
    );
}

// ===========================================================================
// T081 -- Relevance Benchmark: Top-1 Precision
// ===========================================================================

/// Index the rust-sample fixture and run 10 benchmark queries to verify
/// that the top-1 result matches the expected symbol in at least 90% of cases.
#[test]
fn t081_relevance_benchmark_top1_precision() {
    let fixture = fixture_repo_path();
    let tmp = tempdir().expect("tempdir");
    let data_root = tmp.path().join("codecompass-data");
    std::fs::create_dir_all(&data_root).unwrap();

    // -- Init phase --
    let (project_id, data_dir) = do_init(&fixture, &data_root);

    let db_path = data_dir.join(codecompass_core::constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let index_set =
        codecompass_state::tantivy_index::IndexSet::open(&data_dir).expect("open tantivy");
    let effective_ref = codecompass_core::constants::REF_LIVE;

    // -- Index phase: scan, parse, extract, write --
    let files = codecompass_indexer::scanner::scan_directory(
        &fixture,
        codecompass_core::constants::MAX_FILE_SIZE,
    );
    assert!(
        !files.is_empty(),
        "scanner should discover .rs files in the fixture"
    );

    for file in &files {
        let content = std::fs::read_to_string(&file.path).expect("read fixture file");
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let extracted = if codecompass_indexer::parser::is_language_supported(&file.language) {
            match codecompass_indexer::parser::parse_file(&content, &file.language) {
                Ok(tree) => {
                    codecompass_indexer::languages::extract_symbols(&tree, &content, &file.language)
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let symbols = codecompass_indexer::symbol_extract::build_symbol_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let snippets = codecompass_indexer::snippet_extract::build_snippet_records(
            &extracted,
            &project_id,
            effective_ref,
            &file.relative_path,
            None,
        );
        let file_record = codecompass_core::types::FileRecord {
            repo: project_id.clone(),
            r#ref: effective_ref.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash,
            size_bytes: content.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: Some(content.lines().take(20).collect::<Vec<_>>().join("\n")),
        };

        codecompass_state::symbols::delete_symbols_for_file(
            &conn,
            &project_id,
            effective_ref,
            &file.relative_path,
        )
        .unwrap();
        codecompass_indexer::writer::write_file_records(
            &index_set,
            &conn,
            &symbols,
            &snippets,
            &file_record,
        )
        .expect("write_file_records");
    }

    // -- Benchmark queries --
    // Each entry: (query, expected_file_substring)
    // FR-010 requires >= 20 queries at 90% top-1 precision.
    let benchmark_queries: Vec<(&str, &str)> = vec![
        // auth.rs symbols
        ("validate_token", "auth.rs"),
        ("require_role", "auth.rs"),
        ("AuthError", "auth.rs"),
        ("Claims", "auth.rs"),
        ("is_expired", "auth.rs"),
        // handler.rs symbols
        ("AuthHandler", "handler.rs"),
        ("handle_request", "handler.rs"),
        ("Request", "handler.rs"),
        ("Response", "handler.rs"),
        ("Method", "handler.rs"),
        // config.rs symbols
        ("Config", "config.rs"),
        ("ConfigError", "config.rs"),
        ("load", "config.rs"),
        // types.rs symbols
        ("Role", "types.rs"),
        ("User", "types.rs"),
        ("has_role", "types.rs"),
        ("deactivate", "types.rs"),
        // db.rs symbols
        ("DatabaseError", "db.rs"),
        ("Connection", "db.rs"),
        ("execute_in_transaction", "db.rs"),
        // lib.rs symbols
        ("AppState", "lib.rs"),
        ("health_check", "lib.rs"),
    ];

    let total = benchmark_queries.len();
    assert!(
        total >= 20,
        "benchmark must have at least 20 queries, got {}",
        total
    );

    let mut correct = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for (query_name, expected_file) in &benchmark_queries {
        let results = codecompass_query::locate::locate_symbol(
            &index_set.symbols,
            query_name,
            None,
            None,
            None,
            10,
        )
        .unwrap_or_else(|e| panic!("locate_symbol('{}') failed: {}", query_name, e));

        if !results.is_empty() && results[0].path.contains(expected_file) {
            correct += 1;
        } else {
            let top1_info = if results.is_empty() {
                "no results".to_string()
            } else {
                format!("got path='{}' name='{}'", results[0].path, results[0].name)
            };
            failures.push(format!(
                "  MISS: query='{}' expected='{}' -- {}",
                query_name, expected_file, top1_info
            ));
        }
    }

    let precision = (correct as f64) / (total as f64);
    let failure_report = if failures.is_empty() {
        String::new()
    } else {
        format!("\nFailures:\n{}", failures.join("\n"))
    };

    assert!(
        precision >= 0.9,
        "Top-1 precision should be >= 90%, got {:.0}% ({}/{}){}",
        precision * 100.0,
        correct,
        total,
        failure_report,
    );
}
