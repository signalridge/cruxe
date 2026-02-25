use super::*;

pub(super) fn handle_index_operation(params: IndexOperationParams<'_>) -> JsonRpcResponse {
    let IndexOperationParams {
        id,
        tool_name,
        arguments,
        config,
        schema_status,
        conn,
        workspace,
        project_id,
        notifier,
        progress_token,
    } = params;

    let force = arguments
        .get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mode = if force { "full" } else { "incremental" };
    let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
    let metadata = build_metadata(
        &effective_ref,
        schema_status,
        config,
        conn,
        workspace,
        project_id,
    );

    if !is_project_registered(conn, workspace) {
        return tool_error_response(
            id,
            ProtocolErrorCode::ProjectNotFound,
            "Project is not initialized for this workspace. Run `codecompass init` first.",
            Some(json!({
                "workspace": workspace.to_string_lossy(),
                "remediation": "codecompass init --path <workspace>",
            })),
            metadata,
        );
    }
    if has_active_job(conn, project_id) {
        return tool_error_response(
            id,
            ProtocolErrorCode::IndexInProgress,
            "An indexing job is already running.",
            Some(json!({
                "project_id": project_id,
                "remediation": "Use index_status to poll and retry after completion.",
            })),
            metadata,
        );
    }

    let job_id = crate::index_launcher::generate_job_id();
    let server_progress_token = format!("index-job-{}", job_id);
    let effective_progress_token = progress_token
        .clone()
        .filter(|token| !token.trim().is_empty())
        .unwrap_or_else(|| server_progress_token.clone());
    let notifications_enabled = progress_token.is_some();

    let launch_request = crate::index_launcher::IndexLaunchRequest {
        workspace,
        force,
        ref_name: Some(&effective_ref),
        config_path: None,
        project_id: Some(project_id),
        storage_data_dir: Some(&config.storage.data_dir),
        job_id: Some(&job_id),
    };

    match crate::index_launcher::spawn_index_process(&launch_request) {
        Ok(child) => {
            if notifications_enabled {
                notifier.emit_begin(&effective_progress_token, "Indexing", "Starting indexer...");
            }

            let notifier_clone = Arc::clone(&notifier);
            let poll_token = if notifications_enabled {
                Some(effective_progress_token.clone())
            } else {
                None
            };
            let poll_db_path = config
                .project_data_dir(project_id)
                .join(constants::STATE_DB_FILE);
            let poll_project_id = project_id.to_string();
            let notification_start = std::time::Instant::now();
            std::thread::spawn(move || {
                let mut child = child;
                let mut last_scanned = 0i64;
                let mut last_indexed = 0i64;
                let mut last_symbols = 0i64;
                let mut poll_conn = codecompass_state::db::open_connection(&poll_db_path).ok();
                let mut sleep_ms = 1000u64;

                loop {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            if let Some(ref token) = poll_token {
                                let summary = if status.success() {
                                    format!(
                                        "Indexed {} files, {} symbols in {:.1}s",
                                        last_indexed,
                                        last_symbols,
                                        notification_start.elapsed().as_secs_f64(),
                                    )
                                } else {
                                    format!(
                                        "Error: Indexer exited with code {}",
                                        status.code().unwrap_or(-1)
                                    )
                                };
                                let title = if status.success() {
                                    "Indexing complete"
                                } else {
                                    "Indexing failed"
                                };
                                notifier_clone.emit_end(token, title, &summary);
                            }
                            break;
                        }
                        Ok(None) => {
                            if poll_conn.is_none() {
                                poll_conn =
                                    codecompass_state::db::open_connection(&poll_db_path).ok();
                            }

                            let mut progress_changed = false;
                            if let Some(ref token) = poll_token
                                && let Some(conn) = poll_conn.as_ref()
                            {
                                match codecompass_state::jobs::get_active_job(
                                    conn,
                                    &poll_project_id,
                                ) {
                                    Ok(Some(job))
                                        if job.files_scanned != last_scanned
                                            || job.files_indexed != last_indexed
                                            || job.symbols_extracted != last_symbols =>
                                    {
                                        last_scanned = job.files_scanned;
                                        last_indexed = job.files_indexed;
                                        last_symbols = job.symbols_extracted;
                                        progress_changed = true;

                                        let total = last_scanned.max(1);
                                        let pct = ((last_indexed as f64 / total as f64) * 100.0)
                                            .min(99.0)
                                            as u32;
                                        let (msg, stage_pct) = if last_scanned == 0 {
                                            ("Scanning files: 0 discovered".to_string(), 0)
                                        } else if last_indexed == 0 {
                                            (
                                                format!(
                                                    "Scanning files: {} discovered",
                                                    last_scanned
                                                ),
                                                10,
                                            )
                                        } else if pct < 70 {
                                            (
                                                format!(
                                                    "Parsing files: {}/{} ({}%)",
                                                    last_indexed, last_scanned, pct
                                                ),
                                                pct.max(10),
                                            )
                                        } else if pct < 95 {
                                            (
                                                format!(
                                                    "Indexing: {}/{} files, {} symbols",
                                                    last_indexed, last_scanned, last_symbols
                                                ),
                                                pct,
                                            )
                                        } else {
                                            ("Finalizing index...".to_string(), pct.max(95))
                                        };
                                        notifier_clone.emit_progress(
                                            token,
                                            "Indexing",
                                            &msg,
                                            Some(stage_pct.min(99)),
                                        );
                                    }
                                    Ok(_) => {}
                                    Err(_) => {
                                        poll_conn = None;
                                    }
                                }
                            }
                            sleep_ms = if progress_changed {
                                500
                            } else {
                                (sleep_ms + 250).min(2000)
                            };
                            std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
                        }
                        Err(err) => {
                            if let Some(ref token) = poll_token {
                                notifier_clone.emit_end(
                                    token,
                                    "Indexing failed",
                                    &format!("Error: Failed to poll indexer process: {}", err),
                                );
                            }
                            break;
                        }
                    }
                }
            });

            let mut payload = serde_json::Map::new();
            payload.insert("job_id".to_string(), json!(job_id));
            payload.insert("status".to_string(), json!("running"));
            payload.insert("mode".to_string(), json!(mode));
            payload.insert("progress_token".to_string(), json!(server_progress_token));
            if tool_name == "sync_repo" {
                payload.insert("changed_files".to_string(), Value::Null);
            } else {
                payload.insert("file_count".to_string(), Value::Null);
            }
            payload.insert("metadata".to_string(), json!(metadata));
            tool_text_response(id, Value::Object(payload))
        }
        Err(e) => tool_error_response(
            id,
            ProtocolErrorCode::InternalError,
            "Failed to spawn indexer process.",
            Some(json!({
                "details": e.to_string(),
                "remediation": "Run `codecompass index` manually to inspect logs.",
            })),
            metadata,
        ),
    }
}
