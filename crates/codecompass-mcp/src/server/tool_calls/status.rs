use super::*;
use serde::Serialize;

#[derive(Serialize)]
struct IndexStatusActiveJobPayload {
    job_id: String,
    status: String,
    r#ref: String,
    progress_token: Option<String>,
    files_scanned: i64,
    files_indexed: i64,
    symbols_extracted: i64,
    estimated_completion_pct: Option<u32>,
}

#[derive(Serialize)]
struct IndexStatusRecentJobPayload {
    job_id: String,
    r#ref: String,
    mode: String,
    status: String,
    changed_files: i64,
    duration_ms: Option<i64>,
    created_at: String,
}

#[derive(Serialize)]
struct IndexStatusPayload {
    project_id: String,
    repo_root: String,
    index_status: String,
    #[serde(rename = "ref")]
    ref_name: String,
    schema_status: &'static str,
    current_schema_version: u32,
    required_schema_version: u32,
    last_indexed_at: Option<String>,
    file_count: u64,
    symbol_count: u64,
    compatibility_reason: Option<String>,
    active_job: Option<IndexStatusActiveJobPayload>,
    recent_jobs: Vec<IndexStatusRecentJobPayload>,
    interrupted_recovery_report: Option<Value>,
    metadata: ProtocolMetadata,
}

pub(super) fn handle_index_status(params: IndexStatusToolParams<'_>) -> JsonRpcResponse {
    let IndexStatusToolParams {
        id,
        arguments,
        config,
        schema_status,
        compatibility_reason,
        conn,
        workspace,
        project_id,
    } = params;

    let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
    let stored_schema_version = conn.and_then(|c| {
        codecompass_state::project::get_by_id(c, project_id)
            .ok()
            .flatten()
            .map(|p| p.schema_version)
    });
    let (schema_status_str, _) = schema_status_contract(schema_status);
    let current_schema_version =
        schema_status_current_version(schema_status, stored_schema_version.unwrap_or(0));
    let status = if matches!(schema_status, SchemaStatus::Compatible) {
        "ready"
    } else {
        "not_indexed"
    };

    let (file_count, symbol_count) = conn
        .map(|c| {
            let fc =
                codecompass_state::manifest::file_count(c, project_id, &effective_ref).unwrap_or(0);
            let sc = codecompass_state::symbols::symbol_count(c, project_id, &effective_ref)
                .unwrap_or(0);
            (fc, sc)
        })
        .unwrap_or((0, 0));

    let recent_jobs = conn
        .and_then(|c| codecompass_state::jobs::get_recent_jobs(c, project_id, 5).ok())
        .unwrap_or_default();

    let active_job = conn.and_then(|c| {
        codecompass_state::jobs::get_active_job(c, project_id)
            .ok()
            .flatten()
    });

    let last_indexed_at: Option<String> = recent_jobs
        .iter()
        .find(|j| j.status == "published" && j.r#ref == effective_ref)
        .map(|j| j.updated_at.clone());

    let interrupted_recovery_report = build_interrupted_recovery_report(conn);

    let active_job_payload = active_job.map(|j| {
        let total = j.files_scanned.max(1);
        let pct = if j.files_scanned > 0 {
            Some(((j.files_indexed as f64 / total as f64) * 100.0).min(99.0) as u32)
        } else {
            None
        };
        IndexStatusActiveJobPayload {
            job_id: j.job_id,
            status: j.status,
            r#ref: j.r#ref,
            progress_token: j.progress_token,
            files_scanned: j.files_scanned,
            files_indexed: j.files_indexed,
            symbols_extracted: j.symbols_extracted,
            estimated_completion_pct: pct,
        }
    });
    let recent_jobs_payload = recent_jobs
        .iter()
        .map(|j| IndexStatusRecentJobPayload {
            job_id: j.job_id.clone(),
            r#ref: j.r#ref.clone(),
            mode: j.mode.clone(),
            status: j.status.clone(),
            changed_files: j.changed_files,
            duration_ms: j.duration_ms,
            created_at: j.created_at.clone(),
        })
        .collect::<Vec<_>>();
    let result = serde_json::to_value(IndexStatusPayload {
        project_id: project_id.to_string(),
        repo_root: workspace.to_string_lossy().to_string(),
        index_status: status.to_string(),
        ref_name: effective_ref.clone(),
        schema_status: schema_status_str,
        current_schema_version,
        required_schema_version: constants::SCHEMA_VERSION,
        last_indexed_at,
        file_count,
        symbol_count,
        compatibility_reason: compatibility_reason.map(str::to_string),
        active_job: active_job_payload,
        recent_jobs: recent_jobs_payload,
        interrupted_recovery_report,
        metadata: build_metadata(
            &effective_ref,
            schema_status,
            config,
            conn,
            workspace,
            project_id,
        ),
    })
    .unwrap_or_else(|_| json!({"error": "failed to serialize index_status payload"}));
    tool_text_response(id, result)
}
