use cruxe_core::error::StateError;
use cruxe_core::types::JobStatus;
use rusqlite::{Connection, ErrorCode, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexJob {
    pub job_id: String,
    pub project_id: String,
    pub r#ref: String,
    pub mode: String,
    pub head_commit: Option<String>,
    pub sync_id: Option<String>,
    pub status: String,
    pub changed_files: i64,
    pub duration_ms: Option<i64>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub progress_token: Option<String>,
    pub files_scanned: i64,
    pub files_indexed: i64,
    pub symbols_extracted: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Create a new index job.
pub fn create_job(conn: &Connection, job: &IndexJob) -> Result<(), StateError> {
    match conn.execute(
        "INSERT INTO index_jobs (job_id, project_id, \"ref\", mode, head_commit, sync_id, status, changed_files, duration_ms, error_message, retry_count, progress_token, files_scanned, files_indexed, symbols_extracted, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            job.job_id,
            job.project_id,
            job.r#ref,
            job.mode,
            job.head_commit,
            job.sync_id,
            job.status,
            job.changed_files,
            job.duration_ms,
            job.error_message,
            job.retry_count,
            job.progress_token,
            job.files_scanned,
            job.files_indexed,
            job.symbols_extracted,
            job.created_at,
            job.updated_at,
        ],
    ) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == ErrorCode::ConstraintViolation =>
        {
            if let Some(active) = get_active_job_for_ref(conn, &job.project_id, &job.r#ref)? {
                return Err(StateError::sync_in_progress(
                    &job.project_id,
                    &job.r#ref,
                    active.job_id,
                ));
            }
            Err(StateError::Sqlite(
                "index_jobs constraint violation while creating job".to_string(),
            ))
        }
        Err(e) => Err(StateError::sqlite(e)),
    }
}

/// Update job status.
pub fn update_job_status(
    conn: &Connection,
    job_id: &str,
    status: JobStatus,
    changed_files: Option<i64>,
    duration_ms: Option<i64>,
    error_message: Option<&str>,
    updated_at: &str,
) -> Result<(), StateError> {
    conn.execute(
        "UPDATE index_jobs SET status = ?1, changed_files = COALESCE(?2, changed_files), duration_ms = COALESCE(?3, duration_ms), error_message = COALESCE(?4, error_message), updated_at = ?5 WHERE job_id = ?6",
        params![
            status.as_str(),
            changed_files,
            duration_ms,
            error_message,
            updated_at,
            job_id,
        ],
    ).map_err(StateError::sqlite)?;
    Ok(())
}

/// Get the active (running) job for a project, if any.
pub fn get_active_job(conn: &Connection, project_id: &str) -> Result<Option<IndexJob>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT job_id, project_id, \"ref\", mode, head_commit, sync_id, status, changed_files, duration_ms, error_message, retry_count, progress_token, files_scanned, files_indexed, symbols_extracted, created_at, updated_at
         FROM index_jobs WHERE project_id = ?1 AND status IN ('queued', 'running', 'validating')
         ORDER BY created_at DESC LIMIT 1"
    ).map_err(StateError::sqlite)?;

    let result = stmt.query_row(params![project_id], row_to_job);

    match result {
        Ok(job) => Ok(Some(job)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(StateError::sqlite(e)),
    }
}

/// Get the active (queued/running/validating) job for a specific `(project_id, ref)`.
pub fn get_active_job_for_ref(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
) -> Result<Option<IndexJob>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT job_id, project_id, \"ref\", mode, head_commit, sync_id, status, changed_files, duration_ms, error_message, retry_count, progress_token, files_scanned, files_indexed, symbols_extracted, created_at, updated_at
             FROM index_jobs
             WHERE project_id = ?1 AND \"ref\" = ?2 AND status IN ('queued', 'running', 'validating')
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .map_err(StateError::sqlite)?;

    let result = stmt.query_row(params![project_id, ref_name], row_to_job);

    match result {
        Ok(job) => Ok(Some(job)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(StateError::sqlite(e)),
    }
}

/// Get recent jobs for a project.
pub fn get_recent_jobs(
    conn: &Connection,
    project_id: &str,
    limit: usize,
) -> Result<Vec<IndexJob>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT job_id, project_id, \"ref\", mode, head_commit, sync_id, status, changed_files, duration_ms, error_message, retry_count, progress_token, files_scanned, files_indexed, symbols_extracted, created_at, updated_at
         FROM index_jobs WHERE project_id = ?1
         ORDER BY created_at DESC LIMIT ?2"
    ).map_err(StateError::sqlite)?;

    let jobs = stmt
        .query_map(params![project_id, limit], row_to_job)
        .map_err(StateError::sqlite)?;

    jobs.collect::<Result<Vec<_>, _>>()
        .map_err(|e| StateError::Sqlite(e.to_string()))
}

/// Update progress fields for a running job.
pub fn update_progress(
    conn: &Connection,
    job_id: &str,
    files_scanned: i64,
    files_indexed: i64,
    symbols_extracted: i64,
) -> Result<(), StateError> {
    conn.execute(
        "UPDATE index_jobs SET files_scanned = ?1, files_indexed = ?2, symbols_extracted = ?3, updated_at = ?4 WHERE job_id = ?5",
        params![files_scanned, files_indexed, symbols_extracted, cruxe_core::time::now_iso8601(), job_id],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

/// Mark all running/queued jobs as interrupted. Returns the count of affected jobs.
pub fn mark_interrupted_jobs(conn: &Connection) -> Result<usize, StateError> {
    let count = conn
        .execute(
            "UPDATE index_jobs SET status = 'interrupted' WHERE status IN ('queued', 'running', 'validating')",
            [],
        )
        .map_err(StateError::sqlite)?;
    Ok(count)
}

/// Get interrupted jobs (for recovery reporting).
pub fn get_interrupted_jobs(conn: &Connection) -> Result<Vec<IndexJob>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT job_id, project_id, \"ref\", mode, head_commit, sync_id, status, changed_files, duration_ms, error_message, retry_count, progress_token, files_scanned, files_indexed, symbols_extracted, created_at, updated_at
         FROM index_jobs WHERE status = 'interrupted'
         ORDER BY created_at DESC"
    ).map_err(StateError::sqlite)?;

    let jobs = stmt.query_map([], row_to_job).map_err(StateError::sqlite)?;

    jobs.collect::<Result<Vec<_>, _>>()
        .map_err(|e| StateError::Sqlite(e.to_string()))
}

/// Helper to map a row to IndexJob.
fn row_to_job(row: &rusqlite::Row) -> rusqlite::Result<IndexJob> {
    Ok(IndexJob {
        job_id: row.get(0)?,
        project_id: row.get(1)?,
        r#ref: row.get(2)?,
        mode: row.get(3)?,
        head_commit: row.get(4)?,
        sync_id: row.get(5)?,
        status: row.get(6)?,
        changed_files: row.get(7)?,
        duration_ms: row.get(8)?,
        error_message: row.get(9)?,
        retry_count: row.get(10)?,
        progress_token: row.get(11)?,
        files_scanned: row.get(12)?,
        files_indexed: row.get(13)?,
        symbols_extracted: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::schema;
    use cruxe_core::types::Project;
    use tempfile::tempdir;

    fn setup_test_db() -> Connection {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    /// Insert a project so that the foreign key constraint on index_jobs is satisfied.
    fn insert_test_project(conn: &Connection, project_id: &str) {
        let project = Project {
            project_id: project_id.to_string(),
            repo_root: format!("/home/user/{}", project_id),
            display_name: None,
            default_ref: "main".to_string(),
            vcs_mode: true,
            schema_version: 1,
            parser_version: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        crate::project::create_project(conn, &project).unwrap();
    }

    fn sample_job(project_id: &str) -> IndexJob {
        IndexJob {
            job_id: "job_001".to_string(),
            project_id: project_id.to_string(),
            r#ref: "main".to_string(),
            mode: "full".to_string(),
            head_commit: Some("abc123".to_string()),
            sync_id: Some("sync_001".to_string()),
            status: JobStatus::Queued.as_str().to_string(),
            changed_files: 42,
            duration_ms: None,
            error_message: None,
            retry_count: 0,
            progress_token: None,
            files_scanned: 0,
            files_indexed: 0,
            symbols_extracted: 0,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_create_and_get_active_job() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let job = sample_job("proj_1");
        create_job(&conn, &job).unwrap();

        let active = get_active_job(&conn, "proj_1").unwrap();
        assert!(active.is_some());
        let active = active.unwrap();
        assert_eq!(active.job_id, "job_001");
        assert_eq!(active.project_id, "proj_1");
        assert_eq!(active.r#ref, "main");
        assert_eq!(active.mode, "full");
        assert_eq!(active.head_commit, Some("abc123".to_string()));
        assert_eq!(active.sync_id, Some("sync_001".to_string()));
        assert_eq!(active.status, "queued");
        assert_eq!(active.changed_files, 42);
        assert!(active.duration_ms.is_none());
        assert!(active.error_message.is_none());
        assert_eq!(active.retry_count, 0);
    }

    #[test]
    fn test_get_active_job_returns_none_when_no_jobs() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let active = get_active_job(&conn, "proj_1").unwrap();
        assert!(active.is_none());
    }

    #[test]
    fn test_get_active_job_returns_none_for_completed_jobs() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let mut job = sample_job("proj_1");
        job.status = JobStatus::Published.as_str().to_string();
        create_job(&conn, &job).unwrap();

        let active = get_active_job(&conn, "proj_1").unwrap();
        assert!(active.is_none());
    }

    #[test]
    fn test_get_active_job_returns_running_job() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let mut job = sample_job("proj_1");
        job.status = JobStatus::Running.as_str().to_string();
        create_job(&conn, &job).unwrap();

        let active = get_active_job(&conn, "proj_1").unwrap();
        assert!(active.is_some());
        assert_eq!(active.unwrap().status, "running");
    }

    #[test]
    fn test_get_active_job_returns_validating_job() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let mut job = sample_job("proj_1");
        job.status = JobStatus::Validating.as_str().to_string();
        create_job(&conn, &job).unwrap();

        let active = get_active_job(&conn, "proj_1").unwrap();
        assert!(active.is_some());
        assert_eq!(active.unwrap().status, "validating");
    }

    #[test]
    fn test_get_active_job_for_ref_filters_by_ref() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let mut main_job = sample_job("proj_1");
        main_job.job_id = "job_main".to_string();
        main_job.r#ref = "main".to_string();
        main_job.created_at = "2026-01-01T00:00:01Z".to_string();
        main_job.updated_at = "2026-01-01T00:00:01Z".to_string();
        create_job(&conn, &main_job).unwrap();

        let mut feat_job = sample_job("proj_1");
        feat_job.job_id = "job_feat".to_string();
        feat_job.r#ref = "feat/auth".to_string();
        feat_job.created_at = "2026-01-01T00:00:02Z".to_string();
        feat_job.updated_at = "2026-01-01T00:00:02Z".to_string();
        create_job(&conn, &feat_job).unwrap();

        let active_main = get_active_job_for_ref(&conn, "proj_1", "main")
            .unwrap()
            .unwrap();
        assert_eq!(active_main.job_id, "job_main");

        let active_feat = get_active_job_for_ref(&conn, "proj_1", "feat/auth")
            .unwrap()
            .unwrap();
        assert_eq!(active_feat.job_id, "job_feat");

        let missing = get_active_job_for_ref(&conn, "proj_1", "missing").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_create_job_rejects_second_active_job_for_same_ref() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let mut first = sample_job("proj_1");
        first.job_id = "job_active_1".to_string();
        first.r#ref = "feat/auth".to_string();
        first.status = JobStatus::Running.as_str().to_string();
        create_job(&conn, &first).unwrap();

        let mut second = sample_job("proj_1");
        second.job_id = "job_active_2".to_string();
        second.r#ref = "feat/auth".to_string();
        second.status = JobStatus::Queued.as_str().to_string();

        let err = create_job(&conn, &second).unwrap_err();
        assert!(
            matches!(err, StateError::SyncInProgress { .. }),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_update_job_status() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let job = sample_job("proj_1");
        create_job(&conn, &job).unwrap();

        update_job_status(
            &conn,
            "job_001",
            JobStatus::Running,
            None,
            None,
            None,
            "2026-01-01T01:00:00Z",
        )
        .unwrap();

        let active = get_active_job(&conn, "proj_1").unwrap().unwrap();
        assert_eq!(active.status, "running");
        assert_eq!(active.updated_at, "2026-01-01T01:00:00Z");
    }

    #[test]
    fn test_update_job_status_to_published() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let job = sample_job("proj_1");
        create_job(&conn, &job).unwrap();

        update_job_status(
            &conn,
            "job_001",
            JobStatus::Published,
            Some(100),
            Some(5000),
            None,
            "2026-01-01T02:00:00Z",
        )
        .unwrap();

        // Should no longer appear as active
        let active = get_active_job(&conn, "proj_1").unwrap();
        assert!(active.is_none());

        // But should appear in recent jobs
        let recent = get_recent_jobs(&conn, "proj_1", 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].status, "published");
        assert_eq!(recent[0].changed_files, 100);
        assert_eq!(recent[0].duration_ms, Some(5000));
    }

    #[test]
    fn test_update_job_status_with_error() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let job = sample_job("proj_1");
        create_job(&conn, &job).unwrap();

        update_job_status(
            &conn,
            "job_001",
            JobStatus::Failed,
            None,
            Some(1500),
            Some("disk full"),
            "2026-01-01T03:00:00Z",
        )
        .unwrap();

        let recent = get_recent_jobs(&conn, "proj_1", 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].status, "failed");
        assert_eq!(recent[0].error_message, Some("disk full".to_string()));
        assert_eq!(recent[0].duration_ms, Some(1500));
    }

    #[test]
    fn test_get_recent_jobs_ordering() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        // Insert jobs with different created_at timestamps
        for i in 0..5 {
            let mut job = sample_job("proj_1");
            job.job_id = format!("job_{:03}", i);
            job.status = JobStatus::Published.as_str().to_string();
            job.created_at = format!("2026-01-0{}T00:00:00Z", i + 1);
            create_job(&conn, &job).unwrap();
        }

        let recent = get_recent_jobs(&conn, "proj_1", 3).unwrap();
        assert_eq!(recent.len(), 3);
        // Most recent first (ORDER BY created_at DESC)
        assert_eq!(recent[0].job_id, "job_004");
        assert_eq!(recent[1].job_id, "job_003");
        assert_eq!(recent[2].job_id, "job_002");
    }

    #[test]
    fn test_get_recent_jobs_empty() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let recent = get_recent_jobs(&conn, "proj_1", 10).unwrap();
        assert!(recent.is_empty());
    }

    #[test]
    fn test_get_recent_jobs_limit() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        for i in 0..5 {
            let mut job = sample_job("proj_1");
            job.job_id = format!("job_{:03}", i);
            job.created_at = format!("2026-01-0{}T00:00:00Z", i + 1);
            job.status = JobStatus::Published.as_str().to_string();
            create_job(&conn, &job).unwrap();
        }

        let recent = get_recent_jobs(&conn, "proj_1", 2).unwrap();
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn test_get_recent_jobs_scoped_to_project() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");
        insert_test_project(&conn, "proj_2");

        let job1 = sample_job("proj_1");
        create_job(&conn, &job1).unwrap();

        let mut job2 = sample_job("proj_2");
        job2.job_id = "job_002".to_string();
        create_job(&conn, &job2).unwrap();

        let recent = get_recent_jobs(&conn, "proj_1", 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].project_id, "proj_1");
    }

    #[test]
    fn test_create_job_duplicate_id_fails() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let job = sample_job("proj_1");
        create_job(&conn, &job).unwrap();

        let result = create_job(&conn, &job);
        assert!(result.is_err());
    }

    #[test]
    fn test_job_with_no_optional_fields() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let job = IndexJob {
            job_id: "job_min".to_string(),
            project_id: "proj_1".to_string(),
            r#ref: "main".to_string(),
            mode: "incremental".to_string(),
            head_commit: None,
            sync_id: None,
            status: JobStatus::Queued.as_str().to_string(),
            changed_files: 0,
            duration_ms: None,
            error_message: None,
            retry_count: 0,
            progress_token: None,
            files_scanned: 0,
            files_indexed: 0,
            symbols_extracted: 0,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };

        create_job(&conn, &job).unwrap();

        let active = get_active_job(&conn, "proj_1").unwrap().unwrap();
        assert!(active.head_commit.is_none());
        assert!(active.sync_id.is_none());
        assert!(active.duration_ms.is_none());
        assert!(active.error_message.is_none());
    }

    #[test]
    fn test_get_active_job_returns_most_recent() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        // Insert two active (queued) jobs with different timestamps
        let mut job1 = sample_job("proj_1");
        job1.job_id = "job_old".to_string();
        job1.created_at = "2026-01-01T00:00:00Z".to_string();
        create_job(&conn, &job1).unwrap();

        let mut job2 = sample_job("proj_1");
        job2.job_id = "job_new".to_string();
        job2.r#ref = "feat/auth".to_string();
        job2.created_at = "2026-01-02T00:00:00Z".to_string();
        create_job(&conn, &job2).unwrap();

        let active = get_active_job(&conn, "proj_1").unwrap().unwrap();
        // Should return the most recently created active job
        assert_eq!(active.job_id, "job_new");
    }

    #[test]
    fn test_update_preserves_changed_files_when_none() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1");

        let job = sample_job("proj_1"); // changed_files = 42
        create_job(&conn, &job).unwrap();

        // Update status but pass None for changed_files -- COALESCE should keep the original
        update_job_status(
            &conn,
            "job_001",
            JobStatus::Running,
            None,
            None,
            None,
            "2026-01-01T01:00:00Z",
        )
        .unwrap();

        let recent = get_recent_jobs(&conn, "proj_1", 1).unwrap();
        assert_eq!(recent[0].changed_files, 42);
    }
}
