use anyhow::{Context, Result, bail};
use codecompass_core::config::Config;
use codecompass_core::constants;
use codecompass_core::time::now_iso8601;
use codecompass_core::types::{FileRecord, JobStatus, generate_project_id};
use codecompass_core::vcs;
use codecompass_indexer::{
    import_extract, languages, parser, scanner, snippet_extract, symbol_extract, writer,
};
use codecompass_state::{branch_state, db, edges, jobs, manifest, project, symbols, tantivy_index};
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;
use tracing::{info, warn};

pub fn run(
    repo_root: &Path,
    force: bool,
    r#ref: Option<&str>,
    config_file: Option<&Path>,
) -> Result<()> {
    let repo_root = std::fs::canonicalize(repo_root).context("Failed to resolve project path")?;
    let repo_root_str = repo_root.to_string_lossy().to_string();

    let config = Config::load_with_file(Some(&repo_root), config_file)?;
    let project_id = generate_project_id(&repo_root_str);
    let data_dir = config.project_data_dir(&project_id);

    // Open SQLite with configured pragmas
    let db_path = data_dir.join(constants::STATE_DB_FILE);
    let conn = db::open_connection_with_config(
        &db_path,
        config.storage.busy_timeout_ms,
        config.storage.cache_size,
    )?;

    // Verify project exists
    let proj = project::get_by_root(&conn, &repo_root_str)?
        .ok_or_else(|| anyhow::anyhow!("Project not initialized. Run `codecompass init` first."))?;

    // Check for active jobs
    if let Some(active) = jobs::get_active_job(&conn, &project_id)? {
        bail!("Index already in progress: job_id={}", active.job_id);
    }

    // Determine ref: explicit > current HEAD branch > project default
    let effective_ref = r#ref.map(String::from).unwrap_or_else(|| {
        if vcs::is_git_repo(&repo_root) {
            vcs::detect_head_branch(&repo_root).unwrap_or_else(|_| proj.default_ref.clone())
        } else {
            proj.default_ref.clone()
        }
    });

    // Create job (allow MCP wrapper to inject a stable job id)
    let job_id = std::env::var("CODECOMPASS_JOB_ID")
        .ok()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| format!("{:016x}", rand_u64()));
    let now = now_iso8601();
    let job = jobs::IndexJob {
        job_id: job_id.clone(),
        project_id: project_id.clone(),
        r#ref: effective_ref.clone(),
        mode: if force {
            "full".into()
        } else {
            "incremental".into()
        },
        head_commit: None,
        sync_id: None,
        status: "running".into(),
        changed_files: 0,
        duration_ms: None,
        error_message: None,
        retry_count: 0,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    jobs::create_job(&conn, &job)?;

    println!(
        "Indexing {} (ref: {}, mode: {}) ...",
        repo_root_str, effective_ref, job.mode
    );
    let start = Instant::now();
    let index_result: Result<(u64, u64, u64, u64)> = (|| {
        // Open Tantivy indices. In --force mode, recover by rebuilding incompatible indices.
        let index_set = match tantivy_index::IndexSet::open(&data_dir) {
            Ok(set) => set,
            Err(
                codecompass_core::error::StateError::SchemaMigrationRequired { .. }
                | codecompass_core::error::StateError::CorruptManifest(_),
            ) if force => {
                let base_dir = data_dir.join("base");
                if base_dir.exists() {
                    std::fs::remove_dir_all(&base_dir)
                        .context("Failed to remove incompatible Tantivy indices")?;
                }
                tantivy_index::IndexSet::open(&data_dir)?
            }
            Err(e) => return Err(e.into()),
        };

        // Create batch writer — one IndexWriter per index for the entire operation
        let batch = writer::BatchWriter::new(&index_set)?;

        // Begin SQLite transaction for atomicity with Tantivy commit.
        // All SQLite data writes happen inside this transaction. On success,
        // Tantivy is committed first, then the SQLite transaction is committed.
        // On failure, SQLite rolls back and Tantivy changes are discarded (uncommitted).
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(codecompass_core::error::StateError::sqlite)?;

        // For force mode, clear existing index/state for target repo/ref before rebuild
        if force {
            batch.delete_ref_docs(&index_set, &effective_ref);
            conn.execute(
                "DELETE FROM symbol_relations WHERE repo = ?1 AND \"ref\" = ?2",
                (&project_id, &effective_ref),
            )
            .map_err(codecompass_core::error::StateError::sqlite)?;
            conn.execute(
                "DELETE FROM symbol_edges WHERE repo = ?1 AND \"ref\" = ?2",
                (&project_id, &effective_ref),
            )
            .map_err(codecompass_core::error::StateError::sqlite)?;
            conn.execute(
                "DELETE FROM file_manifest WHERE repo = ?1 AND \"ref\" = ?2",
                (&project_id, &effective_ref),
            )
            .map_err(codecompass_core::error::StateError::sqlite)?;
        }

        // Scan files (filtered by configured languages)
        let files = scanner::scan_directory_filtered(
            &repo_root,
            config.index.max_file_size,
            &config.index.languages,
        );
        println!("Found {} source files", files.len());

        let scanned_paths: HashSet<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();
        let mut removed_count = 0u64;
        if !force {
            for entry in manifest::get_all_entries(&conn, &project_id, &effective_ref)? {
                if !scanned_paths.contains(entry.path.as_str()) {
                    batch.delete_file_docs(&index_set, &project_id, &effective_ref, &entry.path);
                    symbols::delete_symbols_for_file(
                        &conn,
                        &project_id,
                        &effective_ref,
                        &entry.path,
                    )?;
                    let source_edge_id = import_extract::source_symbol_id_for_path(&entry.path);
                    edges::delete_edges_for_file(
                        &conn,
                        &project_id,
                        &effective_ref,
                        vec![source_edge_id.as_str()],
                    )?;
                    manifest::delete_manifest(&conn, &project_id, &effective_ref, &entry.path)?;
                    removed_count += 1;
                }
            }
        }

        let mut indexed_count = 0u64;
        let mut symbol_count = 0u64;
        let mut skipped = 0u64;
        let mut pending_imports: Vec<(String, Vec<import_extract::RawImport>)> = Vec::new();

        for file in &files {
            // Read file content
            let content = match std::fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(e) => {
                    warn!(path = %file.relative_path, error = %e, "Failed to read file");
                    skipped += 1;
                    continue;
                }
            };

            // Compute content hash for incremental
            let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

            // Check if file changed (incremental mode)
            if !force {
                if let Ok(Some(existing_hash)) = manifest::get_content_hash(
                    &conn,
                    &project_id,
                    &effective_ref,
                    &file.relative_path,
                ) && existing_hash == content_hash
                {
                    continue; // File unchanged, skip
                }

                // File changed in incremental mode: delete stale Tantivy docs for this file
                batch.delete_file_docs(
                    &index_set,
                    &project_id,
                    &effective_ref,
                    &file.relative_path,
                );
            }

            // Parse with tree-sitter if supported
            let (extracted, raw_imports) = if parser::is_language_supported(&file.language) {
                match parser::parse_file(&content, &file.language) {
                    Ok(tree) => (
                        languages::extract_symbols(&tree, &content, &file.language),
                        import_extract::extract_imports(
                            &tree,
                            &content,
                            &file.language,
                            &file.relative_path,
                        ),
                    ),
                    Err(e) => {
                        warn!(path = %file.relative_path, error = %e, "Parse failed");
                        (Vec::new(), Vec::new())
                    }
                }
            } else {
                (Vec::new(), Vec::new())
            };

            // Build records
            let symbols_for_file = symbol_extract::build_symbol_records(
                &extracted,
                &project_id,
                &effective_ref,
                &file.relative_path,
                None,
            );
            let snippets = snippet_extract::build_snippet_records(
                &extracted,
                &project_id,
                &effective_ref,
                &file.relative_path,
                None,
            );
            let file_record = FileRecord {
                repo: project_id.clone(),
                r#ref: effective_ref.clone(),
                commit: None,
                path: file.relative_path.clone(),
                filename: file
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                language: file.language.clone(),
                content_hash: content_hash.clone(),
                size_bytes: content.len() as u64,
                updated_at: now_iso8601(),
                content_head: Some(content.lines().take(20).collect::<Vec<_>>().join("\n")),
            };
            let mtime_ns = file_mtime_ns(&file.path);

            // Delete old SQLite records for this file
            symbols::delete_symbols_for_file(
                &conn,
                &project_id,
                &effective_ref,
                &file.relative_path,
            )?;
            // Add to Tantivy batch (no commit yet)
            batch.add_symbols(&index_set.symbols, &symbols_for_file)?;
            batch.add_snippets(&index_set.snippets, &snippets)?;
            batch.add_file(&index_set.files, &file_record)?;

            // Write to SQLite (symbols + manifest)
            batch.write_sqlite(&conn, &symbols_for_file, &file_record, mtime_ns)?;
            pending_imports.push((file.relative_path.clone(), raw_imports));

            symbol_count += symbols_for_file.len() as u64;
            indexed_count += 1;
        }

        // Resolve imports after all symbols are written so cross-file lookups can
        // match symbols regardless of scan order.
        for (path, raw_imports) in pending_imports {
            batch.replace_import_edges_for_file(
                &conn,
                &project_id,
                &effective_ref,
                &path,
                raw_imports,
            )?;
        }

        // Commit Tantivy first, then finalize SQLite transaction.
        // If Tantivy fails, rollback SQLite to keep both stores consistent.
        match batch.commit() {
            Ok(()) => {
                conn.execute_batch("COMMIT")
                    .map_err(codecompass_core::error::StateError::sqlite)?;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e.into());
            }
        }

        let changed_files = indexed_count + removed_count;
        let file_count = manifest::file_count(&conn, &project_id, &effective_ref)?;
        let now = now_iso8601();
        let indexed_commit = if vcs::is_git_repo(&repo_root) {
            vcs::detect_head_commit(&repo_root).unwrap_or_else(|_| effective_ref.clone())
        } else {
            effective_ref.clone()
        };
        let branch_entry = branch_state::BranchState {
            repo: project_id.clone(),
            r#ref: effective_ref.clone(),
            merge_base_commit: None,
            last_indexed_commit: indexed_commit,
            overlay_dir: None,
            file_count: file_count as i64,
            created_at: now.clone(),
            last_accessed_at: now,
        };
        branch_state::upsert_branch_state(&conn, &branch_entry)?;

        Ok((indexed_count, skipped, symbol_count, changed_files))
    })();

    match index_result {
        Ok((indexed_count, skipped, symbol_count, changed_files)) => {
            let duration = start.elapsed();
            let duration_ms = duration.as_millis() as i64;

            // Update job status
            jobs::update_job_status(
                &conn,
                &job_id,
                JobStatus::Published,
                Some(changed_files as i64),
                Some(duration_ms),
                None,
                &now_iso8601(),
            )?;

            println!();
            println!("Indexing complete!");
            println!("  Files indexed: {}", indexed_count);
            println!("  Files skipped: {}", skipped);
            println!("  Symbols found: {}", symbol_count);
            println!("  Changed files: {}", changed_files);
            println!("  Duration:      {:.1}s", duration.as_secs_f64());
            println!("  Job ID:        {}", job_id);

            info!(
                indexed_count,
                symbol_count, changed_files, duration_ms, "Indexing complete"
            );
            Ok(())
        }
        Err(err) => {
            // Ensure any open SQLite transaction is rolled back before updating job status
            let _ = conn.execute_batch("ROLLBACK");
            let duration_ms = start.elapsed().as_millis() as i64;
            let error_message = format!("{err:#}");
            let _ = jobs::update_job_status(
                &conn,
                &job_id,
                JobStatus::Failed,
                None,
                Some(duration_ms),
                Some(&error_message),
                &now_iso8601(),
            );
            Err(err)
        }
    }
}

fn rand_u64() -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    hasher.finish()
}

fn file_mtime_ns(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
}
