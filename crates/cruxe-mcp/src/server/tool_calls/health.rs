use super::*;
use serde::Serialize;

#[derive(Serialize)]
struct GrammarStatusPayload {
    available: Vec<&'static str>,
    missing: Vec<&'static str>,
}

#[derive(Serialize)]
struct StartupIndexPayload {
    status: &'static str,
    current_schema_version: u32,
    required_schema_version: u32,
    message: Option<&'static str>,
}

#[derive(Serialize)]
struct StartupChecksPayload {
    index: StartupIndexPayload,
}

#[derive(Serialize)]
struct WarmsetPayload {
    enabled: bool,
    capacity: usize,
    members: Vec<String>,
}

#[derive(Serialize)]
struct HealthCheckPayload {
    status: &'static str,
    version: &'static str,
    uptime_seconds: u64,
    tantivy_ok: bool,
    sqlite_ok: bool,
    sqlite_error: Option<String>,
    prewarm_status: &'static str,
    grammars: GrammarStatusPayload,
    active_job: Option<Value>,
    interrupted_recovery_report: Option<Value>,
    startup_checks: StartupChecksPayload,
    workspace_warmset: WarmsetPayload,
    projects: Vec<Value>,
    metadata: ProtocolMetadata,
}

pub(super) fn handle_health_check(params: &ToolCallParams<'_>) -> JsonRpcResponse {
    let ToolCallParams {
        id,
        arguments,
        config,
        index_set,
        schema_status,
        conn,
        workspace,
        project_id,
        prewarm_status,
        server_start,
        ..
    } = params;

    let workspace_scoped = arguments
        .get("workspace")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    let effective_ref = resolve_tool_ref(None, workspace, *conn, project_id);
    let metadata = build_metadata(
        &effective_ref,
        *schema_status,
        config,
        *conn,
        workspace,
        project_id,
    );

    let pw_status = prewarm_status.load(Ordering::Acquire);
    let pw_label = prewarm_status_label(pw_status);
    let warmset_capacity = crate::server::warmset_capacity();
    let warmset_members =
        crate::server::collect_warmset_members(*conn, workspace, warmset_capacity);
    let warmset_enabled = pw_status != PREWARM_SKIPPED;

    let tantivy_checks = if let Some(idx) = index_set {
        cruxe_state::tantivy_index::check_tantivy_health(idx)
    } else {
        Vec::new()
    };
    let tantivy_ok = !tantivy_checks.is_empty() && tantivy_checks.iter().all(|c| c.ok);

    let (sqlite_ok, sqlite_error) = conn
        .and_then(|c| cruxe_state::db::check_sqlite_health(c).ok())
        .unwrap_or((false, Some("No database connection".into())));

    let supported = cruxe_indexer::parser::supported_languages();
    let mut grammars_available = Vec::new();
    let mut grammars_missing = Vec::new();
    for lang in &supported {
        match cruxe_indexer::parser::get_language(lang) {
            Ok(_) => grammars_available.push(*lang),
            Err(_) => grammars_missing.push(*lang),
        }
    }
    let health_core = build_health_core_payload(HealthCoreRequest {
        config,
        conn: *conn,
        workspace,
        project_id,
        schema_status: *schema_status,
        prewarm_status: pw_status,
        effective_ref: &effective_ref,
        options: HealthCoreOptions {
            workspace_scoped,
            include_freshness_status: true,
            include_extended_active_job_fields: true,
        },
    });

    let uptime_seconds = server_start.elapsed().as_secs();

    let result = serde_json::to_value(HealthCheckPayload {
        status: health_core.overall_status,
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds,
        tantivy_ok,
        sqlite_ok,
        sqlite_error,
        prewarm_status: pw_label,
        grammars: GrammarStatusPayload {
            available: grammars_available,
            missing: grammars_missing,
        },
        active_job: health_core.active_job,
        interrupted_recovery_report: health_core.interrupted_recovery_report,
        startup_checks: StartupChecksPayload {
            index: StartupIndexPayload {
                status: health_core.startup_index_status,
                current_schema_version: health_core.startup_current_schema_version,
                required_schema_version: constants::SCHEMA_VERSION,
                message: health_core.startup_compat_message,
            },
        },
        workspace_warmset: WarmsetPayload {
            enabled: warmset_enabled,
            capacity: warmset_capacity,
            members: if warmset_enabled {
                warmset_members
            } else {
                Vec::new()
            },
        },
        projects: health_core.projects,
        metadata,
    })
    .unwrap_or_else(|_| json!({"error": "failed to serialize health_check payload"}));
    tool_text_response(id.clone(), result)
}
