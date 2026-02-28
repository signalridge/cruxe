use anyhow::{Context, Result, bail};
use cruxe_core::config::Config;
use cruxe_core::constants;
use cruxe_core::ids::new_job_id;
use cruxe_core::time::now_iso8601;
use cruxe_core::types::{FileRecord, JobStatus, generate_project_id};
use cruxe_core::vcs;
use cruxe_indexer::{
    call_extract, embed_writer, import_extract, prepare, scanner,
    sync_incremental::{self, IncrementalSyncRequest},
    writer,
};
use cruxe_state::{
    branch_state, db, edges, jobs, manifest, project, schema, symbols, tantivy_index,
};
use cruxe_vcs::Git2VcsAdapter;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;
use tracing::{info, warn};

const PROGRESS_UPDATE_EVERY: u64 = 100;
const INDEX_PARALLELISM_ENV: &str = "CRUXE_INDEX_PARALLELISM";

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
    let mut conn = db::open_connection_with_config(
        &db_path,
        config.storage.busy_timeout_ms,
        config.storage.cache_size,
    )?;
    schema::create_tables(&conn)?;

    // Verify project exists
    let proj = project::get_by_root(&conn, &repo_root_str)?
        .ok_or_else(|| anyhow::anyhow!("Project not initialized. Run `cruxe init` first."))?;

    // Check for active jobs
    if let Some(active) = jobs::get_active_job(&conn, &project_id)? {
        bail!("Index already in progress: job_id={}", active.job_id);
    }

    // Determine ref: explicit > current HEAD branch > project default
    let effective_ref = vcs::resolve_effective_ref(&repo_root, r#ref, &proj.default_ref);

    // VCS mode non-default refs use spec-005 overlay incremental sync path.
    if proj.vcs_mode && effective_ref != proj.default_ref {
        let last_indexed_commit =
            branch_state::get_branch_state(&conn, &project_id, &effective_ref)?
                .map(|state| state.last_indexed_commit);
        let sync_id = format!("sync-{}", new_job_id());
        let adapter = Git2VcsAdapter;
        println!(
            "Syncing overlay {} (base: {}) ...",
            effective_ref, proj.default_ref
        );
        let started = Instant::now();
        let stats = sync_incremental::run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: &project_id,
                ref_name: &effective_ref,
                base_ref: &proj.default_ref,
                sync_id: &sync_id,
                last_indexed_commit: last_indexed_commit.as_deref(),
                is_default_branch: false,
            },
        )?;
        println!();
        println!("Overlay sync complete!");
        println!("  Changed files:  {}", stats.changed_files);
        println!("  Processed files: {}", stats.processed_files);
        println!("  Symbols written: {}", stats.symbols_written);
        println!("  Rebuild:        {}", stats.rebuild_triggered);
        println!("  Duration:       {:.1}s", started.elapsed().as_secs_f64());
        return Ok(());
    }

    // Create job (allow MCP wrapper to inject a stable job id)
    let job_id = std::env::var("CRUXE_JOB_ID")
        .ok()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(new_job_id);
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
        progress_token: Some(format!("index-job-{}", job_id)),
        files_scanned: 0,
        files_indexed: 0,
        symbols_extracted: 0,
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
                cruxe_core::error::StateError::SchemaMigrationRequired { .. }
                | cruxe_core::error::StateError::CorruptManifest(_),
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

        // Create batch writer — one IndexWriter per index for the entire operation.
        // NOTE: SQLite writes are auto-committed (no explicit transaction) so that
        // progress updates in `index_jobs` are immediately visible to `index_status`
        // polling from the MCP server.  Tantivy is committed at the end via
        // `batch.commit()`.  A crash mid-indexing may leave partial SQLite data, but
        // the next incremental or force index run will reconcile both stores.
        let batch = writer::BatchWriter::new(&index_set)?;
        let mut embedding_writer = embed_writer::EmbeddingWriter::new(
            &config.search.semantic,
            &project_id,
            &effective_ref,
        )?;

        // For force mode, clear existing index/state for target repo/ref before rebuild
        if force {
            batch.delete_ref_docs(&index_set, &effective_ref);
            embedding_writer.delete_for_ref(&conn)?;
            conn.execute(
                "DELETE FROM symbol_relations WHERE repo = ?1 AND \"ref\" = ?2",
                (&project_id, &effective_ref),
            )
            .map_err(cruxe_core::error::StateError::sqlite)?;
            conn.execute(
                "DELETE FROM symbol_edges WHERE repo = ?1 AND \"ref\" = ?2",
                (&project_id, &effective_ref),
            )
            .map_err(cruxe_core::error::StateError::sqlite)?;
            conn.execute(
                "DELETE FROM file_manifest WHERE repo = ?1 AND \"ref\" = ?2",
                (&project_id, &effective_ref),
            )
            .map_err(cruxe_core::error::StateError::sqlite)?;
        }

        // Scan files (filtered by configured languages)
        let files = scanner::scan_directory_filtered(
            &repo_root,
            config.index.max_file_size,
            &config.index.languages,
        );
        let total_scanned = files.len() as i64;
        if let Err(err) = jobs::update_progress(&conn, &job_id, total_scanned, 0, 0) {
            warn!(job_id = %job_id, "Failed to update index progress: {}", err);
        }
        println!("Found {} source files", files.len());

        let scanned_paths: HashSet<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();
        let existing_manifest_entries = if force {
            Vec::new()
        } else {
            manifest::get_all_entries(&conn, &project_id, &effective_ref)?
        };
        let existing_hashes: HashMap<String, String> = existing_manifest_entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.content_hash.clone()))
            .collect();

        let mut removed_count = 0u64;
        if !force {
            for entry in &existing_manifest_entries {
                if !scanned_paths.contains(entry.path.as_str()) {
                    let deleted_symbol_ids: Vec<String> = symbols::list_symbols_in_file(
                        &conn,
                        &project_id,
                        &effective_ref,
                        &entry.path,
                    )?
                    .into_iter()
                    .map(|symbol| symbol.symbol_stable_id)
                    .collect();
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
                    edges::delete_call_edges_for_file(
                        &conn,
                        &project_id,
                        &effective_ref,
                        &entry.path,
                    )?;
                    edges::delete_call_edges_to_symbols(
                        &conn,
                        &project_id,
                        &effective_ref,
                        &deleted_symbol_ids,
                    )?;
                    embedding_writer.delete_for_file_vectors_with_symbols(
                        &conn,
                        &entry.path,
                        &deleted_symbol_ids,
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
        let mut pending_call_edges: Vec<(String, Vec<cruxe_core::types::CallEdge>)> = Vec::new();

        let parallelism = resolve_index_parallelism();
        let chunk_size = std::cmp::max(parallelism * 8, PROGRESS_UPDATE_EVERY as usize);
        let worker_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(parallelism)
            .thread_name(|idx| format!("cruxe-index-{idx}"))
            .build()
            .context("Failed to initialize index worker pool")?;

        println!(
            "Using {} indexing worker(s) for read/parse/extract",
            parallelism
        );

        for file_chunk in files.chunks(chunk_size) {
            let prepared_chunk: Vec<PreparedIndexOutcome> = worker_pool.install(|| {
                file_chunk
                    .par_iter()
                    .map(|file| {
                        prepare_file_for_indexing(
                            file,
                            &project_id,
                            &effective_ref,
                            force,
                            existing_hashes.get(&file.relative_path).map(String::as_str),
                        )
                    })
                    .collect()
            });

            let mut pending_embedding_batches = Vec::new();
            for prepared in prepared_chunk {
                match prepared {
                    PreparedIndexOutcome::Unchanged => {}
                    PreparedIndexOutcome::SkippedRead { path, error } => {
                        warn!(path = %path, error = %error, "Failed to read file");
                        skipped += 1;
                    }
                    PreparedIndexOutcome::Ready(prepared) => {
                        let PreparedIndexFile {
                            symbols_for_file,
                            snippets,
                            raw_imports,
                            call_edges,
                            file_record,
                            mtime_ns,
                            parse_error,
                            had_previous_index,
                        } = *prepared;

                        if let Some(parse_error) = parse_error.as_deref() {
                            warn!(
                                path = %file_record.path,
                                error = %parse_error,
                                "Parse failed"
                            );
                        }

                        if !force {
                            batch.delete_file_docs(
                                &index_set,
                                &project_id,
                                &effective_ref,
                                &file_record.path,
                            );
                        }

                        if had_previous_index {
                            embedding_writer.delete_for_file_vectors(&conn, &file_record.path)?;
                        }

                        symbols::delete_symbols_for_file(
                            &conn,
                            &project_id,
                            &effective_ref,
                            &file_record.path,
                        )?;
                        batch.add_symbols(&index_set.symbols, &symbols_for_file)?;
                        batch.add_snippets(&index_set.snippets, &snippets)?;
                        batch.add_file(&index_set.files, &file_record)?;
                        batch.write_sqlite(&conn, &symbols_for_file, &file_record, mtime_ns)?;

                        let symbol_delta = symbols_for_file.len() as u64;
                        pending_imports.push((file_record.path.clone(), raw_imports));
                        pending_call_edges.push((file_record.path.clone(), call_edges));
                        pending_embedding_batches.push((symbols_for_file, snippets));

                        symbol_count += symbol_delta;
                        indexed_count += 1;
                        if indexed_count.is_multiple_of(PROGRESS_UPDATE_EVERY)
                            && let Err(err) = jobs::update_progress(
                                &conn,
                                &job_id,
                                total_scanned,
                                indexed_count as i64,
                                symbol_count as i64,
                            )
                        {
                            warn!(job_id = %job_id, "Failed to update index progress: {}", err);
                        }
                    }
                }
            }

            embedding_writer.write_embeddings_for_files(
                &conn,
                pending_embedding_batches
                    .iter()
                    .map(|(symbols, snippets)| (symbols.as_slice(), snippets.as_slice())),
            )?;
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
        if !pending_call_edges.is_empty() {
            let lookup = call_extract::load_symbol_lookup(&conn, &project_id, &effective_ref)?;
            for (_, call_edges) in pending_call_edges.iter_mut() {
                call_extract::resolve_call_targets_with_lookup(&lookup, call_edges);
            }
            batch.replace_call_edges_for_files(
                &conn,
                &project_id,
                &effective_ref,
                pending_call_edges,
            )?;
        }

        // Commit Tantivy segment updates.
        match batch.commit() {
            Ok(()) => {}
            Err(e) => {
                return Err(e.into());
            }
        }

        let changed_files = indexed_count + removed_count;
        let file_count = manifest::file_count(&conn, &project_id, &effective_ref)?;
        let total_symbol_count = symbols::symbol_count(&conn, &project_id, &effective_ref)?;
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
            symbol_count: total_symbol_count as i64,
            is_default_branch: effective_ref == proj.default_ref,
            status: "active".to_string(),
            eviction_eligible_at: None,
            created_at: now.clone(),
            last_accessed_at: now,
        };
        branch_state::upsert_branch_state(&conn, &branch_entry)?;
        if let Err(err) = jobs::update_progress(
            &conn,
            &job_id,
            total_scanned,
            indexed_count as i64,
            symbol_count as i64,
        ) {
            warn!(job_id = %job_id, "Failed to update index progress: {}", err);
        }

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

fn file_mtime_ns(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
}

struct PreparedIndexFile {
    symbols_for_file: Vec<cruxe_core::types::SymbolRecord>,
    snippets: Vec<cruxe_core::types::SnippetRecord>,
    raw_imports: Vec<import_extract::RawImport>,
    call_edges: Vec<cruxe_core::types::CallEdge>,
    file_record: FileRecord,
    mtime_ns: Option<i64>,
    parse_error: Option<String>,
    had_previous_index: bool,
}

enum PreparedIndexOutcome {
    Unchanged,
    SkippedRead { path: String, error: String },
    Ready(Box<PreparedIndexFile>),
}

fn prepare_file_for_indexing(
    file: &scanner::ScannedFile,
    project_id: &str,
    effective_ref: &str,
    force: bool,
    existing_hash: Option<&str>,
) -> PreparedIndexOutcome {
    let content = match std::fs::read_to_string(&file.path) {
        Ok(c) => c,
        Err(err) => {
            return PreparedIndexOutcome::SkippedRead {
                path: file.relative_path.clone(),
                error: err.to_string(),
            };
        }
    };

    let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();
    let had_previous_index = existing_hash.is_some();
    if !force && existing_hash == Some(content_hash.as_str()) {
        return PreparedIndexOutcome::Unchanged;
    }

    let artifacts = prepare::build_source_artifacts(
        &content,
        &file.language,
        &file.relative_path,
        project_id,
        effective_ref,
        None,
        true,
    );
    let filename = file
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut file_record = prepare::build_file_record(
        project_id,
        effective_ref,
        &file.relative_path,
        &filename,
        &file.language,
        &content,
    );
    // Reuse the precomputed hash used for unchanged short-circuit checks.
    file_record.content_hash = content_hash;

    PreparedIndexOutcome::Ready(Box::new(PreparedIndexFile {
        symbols_for_file: artifacts.symbols,
        snippets: artifacts.snippets,
        raw_imports: artifacts.raw_imports,
        call_edges: artifacts.call_edges,
        file_record,
        mtime_ns: file_mtime_ns(&file.path),
        parse_error: artifacts.parse_error,
        had_previous_index,
    }))
}

fn resolve_index_parallelism() -> usize {
    std::env::var(INDEX_PARALLELISM_ENV)
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(std::num::NonZeroUsize::get)
                .unwrap_or(1)
        })
}
