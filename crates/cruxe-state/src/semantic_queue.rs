use cruxe_core::error::StateError;
use rusqlite::{Connection, OptionalExtension, params};

const STATUS_PENDING: &str = "pending";
const STATUS_RUNNING: &str = "running";
const STATUS_DONE: &str = "done";
const STATUS_FAILED: &str = "failed";
const ERROR_SUPERSEDED: &str = "superseded";
const MAX_RETRY_COUNT: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticEnrichmentJob {
    pub id: i64,
    pub project_id: String,
    pub ref_name: String,
    pub path: String,
    pub generation: i64,
    pub retry_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticEnrichmentRuntimeState {
    pub semantic_enrichment_state: String,
    pub semantic_backlog_size: usize,
    pub semantic_lag_hint: String,
    pub degraded_reason: Option<String>,
}

impl Default for SemanticEnrichmentRuntimeState {
    fn default() -> Self {
        Self {
            semantic_enrichment_state: "ready".to_string(),
            semantic_backlog_size: 0,
            semantic_lag_hint: "none".to_string(),
            degraded_reason: None,
        }
    }
}

pub fn enqueue_file_update(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    path: &str,
) -> Result<i64, StateError> {
    conn.execute_batch("SAVEPOINT semantic_queue_enqueue")
        .map_err(StateError::sqlite)?;
    let result = (|| -> Result<i64, StateError> {
        let latest_generation: Option<i64> = conn
            .query_row(
                "SELECT MAX(generation)
                 FROM semantic_enrichment_queue
                 WHERE project_id = ?1 AND \"ref\" = ?2 AND path = ?3",
                params![project_id, ref_name, path],
                |row| row.get(0),
            )
            .optional()
            .map_err(StateError::sqlite)?
            .flatten();
        let next_generation = latest_generation.unwrap_or(0) + 1;

        conn.execute(
            "INSERT INTO semantic_enrichment_queue (
                 project_id, \"ref\", path, generation, status,
                 retry_count, last_error_code, enqueued_at, started_at, completed_at, next_attempt_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 0, NULL, datetime('now'), NULL, NULL, NULL)",
            params![project_id, ref_name, path, next_generation, STATUS_PENDING],
        )
        .map_err(StateError::sqlite)?;

        // latest-wins: older pending/running generations are superseded.
        conn.execute(
            "UPDATE semantic_enrichment_queue
             SET status = ?1,
                 completed_at = datetime('now'),
                 last_error_code = ?2
             WHERE project_id = ?3
               AND \"ref\" = ?4
               AND path = ?5
               AND generation < ?6
               AND status IN (?7, ?8)",
            params![
                STATUS_DONE,
                ERROR_SUPERSEDED,
                project_id,
                ref_name,
                path,
                next_generation,
                STATUS_PENDING,
                STATUS_RUNNING
            ],
        )
        .map_err(StateError::sqlite)?;

        Ok(next_generation)
    })();

    match result {
        Ok(generation) => {
            conn.execute_batch("RELEASE SAVEPOINT semantic_queue_enqueue")
                .map_err(StateError::sqlite)?;
            Ok(generation)
        }
        Err(err) => {
            let _ = conn.execute_batch(
                "ROLLBACK TO SAVEPOINT semantic_queue_enqueue;
                 RELEASE SAVEPOINT semantic_queue_enqueue;",
            );
            Err(err)
        }
    }
}

pub fn dequeue_pending_jobs(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    limit: usize,
) -> Result<Vec<SemanticEnrichmentJob>, StateError> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut stmt = conn
        .prepare(
            "WITH candidates AS (
                 SELECT q.id
                 FROM semantic_enrichment_queue q
                 WHERE q.project_id = ?1
                   AND q.\"ref\" = ?2
                   AND q.status = ?3
                   AND (
                        q.next_attempt_at IS NULL
                        OR q.next_attempt_at <= datetime('now')
                   )
                   AND q.generation = (
                        SELECT MAX(generation)
                        FROM semantic_enrichment_queue
                        WHERE project_id = q.project_id
                          AND \"ref\" = q.\"ref\"
                          AND path = q.path
                   )
                 ORDER BY q.enqueued_at ASC, q.id ASC
                 LIMIT ?4
             )
             UPDATE semantic_enrichment_queue
             SET status = ?5,
                 started_at = COALESCE(started_at, datetime('now'))
             WHERE id IN (SELECT id FROM candidates)
               AND status = ?3
             RETURNING id, project_id, \"ref\", path, generation, retry_count, enqueued_at",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map(
            params![
                project_id,
                ref_name,
                STATUS_PENDING,
                limit.max(1),
                STATUS_RUNNING
            ],
            |row| {
                Ok((
                    SemanticEnrichmentJob {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        ref_name: row.get(2)?,
                        path: row.get(3)?,
                        generation: row.get(4)?,
                        retry_count: row.get(5)?,
                    },
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .map_err(StateError::sqlite)?;

    let mut claimed = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)?;
    claimed.sort_by(
        |(left_job, left_enqueued_at), (right_job, right_enqueued_at)| {
            left_enqueued_at
                .cmp(right_enqueued_at)
                .then_with(|| left_job.id.cmp(&right_job.id))
        },
    );
    Ok(claimed.into_iter().map(|(job, _enqueued_at)| job).collect())
}

pub fn mark_job_done(conn: &Connection, job_id: i64) -> Result<bool, StateError> {
    let updated = conn
        .execute(
            "UPDATE semantic_enrichment_queue
             SET status = ?1,
                 started_at = COALESCE(started_at, datetime('now')),
                 completed_at = datetime('now'),
                 last_error_code = NULL,
                 next_attempt_at = NULL
             WHERE id = ?2
               AND status IN (?3, ?4)",
            params![STATUS_DONE, job_id, STATUS_PENDING, STATUS_RUNNING],
        )
        .map_err(StateError::sqlite)?;
    Ok(updated == 1)
}

pub fn mark_job_retry_or_failed(
    conn: &Connection,
    job_id: i64,
    error_code: &str,
    backoff_seconds: i64,
    max_retry_count: i64,
) -> Result<bool, StateError> {
    let retry_cap = max_retry_count.max(0);
    let updated = conn
        .execute(
            "UPDATE semantic_enrichment_queue
             SET status = CASE
                 WHEN retry_count < ?1 THEN ?2
                 ELSE ?3
               END,
               retry_count = CASE
                 WHEN retry_count < ?1 THEN retry_count + 1
                 ELSE retry_count
               END,
               started_at = COALESCE(started_at, datetime('now')),
               completed_at = CASE
                 WHEN retry_count < ?1 THEN NULL
                 ELSE datetime('now')
               END,
               next_attempt_at = CASE
                 WHEN retry_count < ?1 THEN datetime('now', printf('+%d seconds', ?4))
                 ELSE NULL
               END,
               last_error_code = ?5
             WHERE id = ?6
               AND status IN (?7, ?8)",
            params![
                retry_cap,
                STATUS_PENDING,
                STATUS_FAILED,
                backoff_seconds.max(1),
                error_code,
                job_id,
                STATUS_PENDING,
                STATUS_RUNNING
            ],
        )
        .map_err(StateError::sqlite)?;
    Ok(updated == 1)
}

pub fn mark_latest_done(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    path: &str,
) -> Result<usize, StateError> {
    conn.execute(
        "UPDATE semantic_enrichment_queue
         SET status = ?1,
             started_at = COALESCE(started_at, datetime('now')),
             completed_at = datetime('now'),
             last_error_code = NULL,
             next_attempt_at = NULL
         WHERE project_id = ?2
           AND \"ref\" = ?3
           AND path = ?4
           AND generation = (
             SELECT MAX(generation)
             FROM semantic_enrichment_queue
             WHERE project_id = ?2 AND \"ref\" = ?3 AND path = ?4
           )
           AND status IN (?5, ?6)",
        params![
            STATUS_DONE,
            project_id,
            ref_name,
            path,
            STATUS_PENDING,
            STATUS_RUNNING
        ],
    )
    .map_err(StateError::sqlite)
}

pub fn mark_latest_failed(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    path: &str,
    error_code: &str,
    backoff_seconds: i64,
) -> Result<usize, StateError> {
    // Retry a bounded number of times before terminal failure.
    let updated = conn
        .execute(
            "UPDATE semantic_enrichment_queue
             SET status = CASE
                 WHEN retry_count < ?1 THEN ?2
                 ELSE ?3
               END,
               retry_count = CASE
                 WHEN retry_count < ?1 THEN retry_count + 1
                 ELSE retry_count
               END,
               started_at = COALESCE(started_at, datetime('now')),
               completed_at = CASE
                 WHEN retry_count < ?1 THEN NULL
                 ELSE datetime('now')
               END,
               next_attempt_at = CASE
                 WHEN retry_count < ?1 THEN datetime('now', printf('+%d seconds', ?4))
                 ELSE NULL
               END,
               last_error_code = ?5
             WHERE project_id = ?6
               AND \"ref\" = ?7
               AND path = ?8
               AND generation = (
                 SELECT MAX(generation)
                 FROM semantic_enrichment_queue
                 WHERE project_id = ?6 AND \"ref\" = ?7 AND path = ?8
               )
               AND status IN (?9, ?10)",
            params![
                MAX_RETRY_COUNT,
                STATUS_PENDING,
                STATUS_FAILED,
                backoff_seconds.max(1),
                error_code,
                project_id,
                ref_name,
                path,
                STATUS_PENDING,
                STATUS_RUNNING
            ],
        )
        .map_err(StateError::sqlite)?;
    Ok(updated)
}

pub fn current_runtime_state(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
) -> Result<SemanticEnrichmentRuntimeState, StateError> {
    let backlog_size: usize = conn
        .query_row(
            "SELECT COUNT(*)
             FROM semantic_enrichment_queue
             WHERE project_id = ?1
               AND \"ref\" = ?2
               AND status IN (?3, ?4)",
            params![project_id, ref_name, STATUS_PENDING, STATUS_RUNNING],
            |row| row.get(0),
        )
        .map_err(StateError::sqlite)?;

    let max_lag_minutes: Option<i64> = conn
        .query_row(
            "SELECT MAX(CAST((julianday('now') - julianday(enqueued_at)) * 24 * 60 AS INTEGER))
             FROM semantic_enrichment_queue
             WHERE project_id = ?1
               AND \"ref\" = ?2
               AND status IN (?3, ?4)",
            params![project_id, ref_name, STATUS_PENDING, STATUS_RUNNING],
            |row| row.get(0),
        )
        .optional()
        .map_err(StateError::sqlite)?
        .flatten();

    let recent_failed: usize = conn
        .query_row(
            "SELECT COUNT(*)
             FROM semantic_enrichment_queue
             WHERE project_id = ?1
               AND \"ref\" = ?2
               AND status = ?3
               AND completed_at >= datetime('now', '-1 hour')",
            params![project_id, ref_name, STATUS_FAILED],
            |row| row.get(0),
        )
        .map_err(StateError::sqlite)?;

    let lag_hint = match max_lag_minutes.unwrap_or(0) {
        minutes if minutes >= 30 => "high",
        minutes if minutes >= 5 => "medium",
        minutes if minutes > 0 => "low",
        _ => "none",
    }
    .to_string();

    let mut state = SemanticEnrichmentRuntimeState {
        semantic_enrichment_state: "ready".to_string(),
        semantic_backlog_size: backlog_size,
        semantic_lag_hint: lag_hint,
        degraded_reason: None,
    };

    if recent_failed > 0 {
        state.semantic_enrichment_state = "degraded".to_string();
        state.degraded_reason = Some("recent_worker_failures".to_string());
    } else if backlog_size >= 128 {
        state.semantic_enrichment_state = "degraded".to_string();
        state.degraded_reason = Some("backlog_exceeded_degraded_threshold".to_string());
    } else if backlog_size > 0 {
        state.semantic_enrichment_state = "backlog".to_string();
    }

    Ok(state)
}

pub fn cleanup_terminal_rows(
    conn: &Connection,
    done_ttl_hours: i64,
    failed_ttl_hours: i64,
    batch_limit: usize,
) -> Result<usize, StateError> {
    let done_deleted = conn
        .execute(
            "DELETE FROM semantic_enrichment_queue
             WHERE rowid IN (
               SELECT rowid
               FROM semantic_enrichment_queue
               WHERE status = ?1
                 AND completed_at IS NOT NULL
                 AND completed_at < datetime('now', printf('-%d hours', ?2))
               ORDER BY completed_at ASC
               LIMIT ?3
             )",
            params![STATUS_DONE, done_ttl_hours.max(1), batch_limit.max(1)],
        )
        .map_err(StateError::sqlite)?;

    let failed_deleted = conn
        .execute(
            "DELETE FROM semantic_enrichment_queue
             WHERE rowid IN (
               SELECT rowid
               FROM semantic_enrichment_queue
               WHERE status = ?1
                 AND completed_at IS NOT NULL
                 AND completed_at < datetime('now', printf('-%d hours', ?2))
               ORDER BY completed_at ASC
               LIMIT ?3
             )",
            params![STATUS_FAILED, failed_ttl_hours.max(1), batch_limit.max(1)],
        )
        .map_err(StateError::sqlite)?;

    Ok(done_deleted + failed_deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, schema};
    use tempfile::tempdir;

    fn setup_conn() -> Connection {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn enqueue_uses_latest_wins_generations() {
        let conn = setup_conn();
        let g1 = enqueue_file_update(&conn, "p1", "main", "src/lib.rs").unwrap();
        let g2 = enqueue_file_update(&conn, "p1", "main", "src/lib.rs").unwrap();
        assert_eq!(g1, 1);
        assert_eq!(g2, 2);

        let statuses: Vec<(i64, String, Option<String>)> = conn
            .prepare(
                "SELECT generation, status, last_error_code
                 FROM semantic_enrichment_queue
                 WHERE project_id = 'p1' AND \"ref\" = 'main' AND path = 'src/lib.rs'
                 ORDER BY generation",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].0, 1);
        assert_eq!(statuses[0].1, STATUS_DONE);
        assert_eq!(statuses[0].2.as_deref(), Some(ERROR_SUPERSEDED));
        assert_eq!(statuses[1].0, 2);
        assert_eq!(statuses[1].1, STATUS_PENDING);
    }

    #[test]
    fn runtime_state_reports_backlog_and_degraded() {
        let conn = setup_conn();
        enqueue_file_update(&conn, "p1", "main", "src/a.rs").unwrap();

        let state = current_runtime_state(&conn, "p1", "main").unwrap();
        assert_eq!(state.semantic_enrichment_state, "backlog");
        assert_eq!(state.semantic_backlog_size, 1);

        for _ in 0..4 {
            mark_latest_failed(&conn, "p1", "main", "src/a.rs", "worker_timeout", 1).unwrap();
        }
        let state = current_runtime_state(&conn, "p1", "main").unwrap();
        assert_eq!(state.semantic_enrichment_state, "degraded");
        assert_eq!(
            state.degraded_reason.as_deref(),
            Some("recent_worker_failures")
        );
    }

    #[test]
    fn dequeue_claims_latest_generation_only() {
        let conn = setup_conn();
        enqueue_file_update(&conn, "p1", "main", "src/lib.rs").unwrap();
        let generation = enqueue_file_update(&conn, "p1", "main", "src/lib.rs").unwrap();
        enqueue_file_update(&conn, "p1", "main", "src/other.rs").unwrap();

        let jobs = dequeue_pending_jobs(&conn, "p1", "main", 10).unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(
            jobs.iter()
                .any(|job| job.path == "src/lib.rs" && job.generation == generation)
        );

        let statuses: Vec<String> = conn
            .prepare(
                "SELECT status
                 FROM semantic_enrichment_queue
                 WHERE project_id = 'p1' AND \"ref\" = 'main' AND status = 'running'",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(statuses.len(), 2);
    }

    #[test]
    fn dequeue_returns_fifo_order_for_claimed_jobs() {
        let conn = setup_conn();
        enqueue_file_update(&conn, "p1", "main", "src/a.rs").unwrap();
        enqueue_file_update(&conn, "p1", "main", "src/b.rs").unwrap();
        enqueue_file_update(&conn, "p1", "main", "src/c.rs").unwrap();

        let jobs = dequeue_pending_jobs(&conn, "p1", "main", 10).unwrap();
        let paths: Vec<&str> = jobs.iter().map(|job| job.path.as_str()).collect();
        assert_eq!(paths, vec!["src/a.rs", "src/b.rs", "src/c.rs"]);
    }

    #[test]
    fn retry_path_transitions_to_failed_after_cap() {
        let conn = setup_conn();
        enqueue_file_update(&conn, "p1", "main", "src/retry.rs").unwrap();
        let job = dequeue_pending_jobs(&conn, "p1", "main", 1)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert!(mark_job_retry_or_failed(&conn, job.id, "embedding_timeout", 1, 2).unwrap());
        assert!(mark_job_retry_or_failed(&conn, job.id, "embedding_timeout", 1, 2).unwrap());
        assert!(mark_job_retry_or_failed(&conn, job.id, "embedding_timeout", 1, 2).unwrap());

        let status: String = conn
            .query_row(
                "SELECT status FROM semantic_enrichment_queue WHERE id = ?1",
                params![job.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, STATUS_FAILED);
    }

    #[test]
    fn dequeue_respects_next_attempt_at_backoff_window() {
        let conn = setup_conn();
        enqueue_file_update(&conn, "p1", "main", "src/backoff.rs").unwrap();
        let job = dequeue_pending_jobs(&conn, "p1", "main", 1)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        mark_job_retry_or_failed(&conn, job.id, "worker_timeout", 300, 3).unwrap();

        let jobs = dequeue_pending_jobs(&conn, "p1", "main", 10).unwrap();
        assert!(jobs.is_empty());
    }

    #[test]
    fn cleanup_prunes_terminal_rows() {
        let conn = setup_conn();
        enqueue_file_update(&conn, "p1", "main", "src/a.rs").unwrap();
        mark_latest_done(&conn, "p1", "main", "src/a.rs").unwrap();
        conn.execute(
            "UPDATE semantic_enrichment_queue
             SET completed_at = datetime('now', '-48 hours')
             WHERE project_id = 'p1' AND \"ref\" = 'main' AND path = 'src/a.rs'",
            [],
        )
        .unwrap();

        let deleted = cleanup_terminal_rows(&conn, 24, 24 * 7, 32).unwrap();
        assert_eq!(deleted, 1);
    }
}
