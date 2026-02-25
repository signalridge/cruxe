use anyhow::{Context, Result};
use codecompass_core::config::Config;
use codecompass_core::constants;
use codecompass_core::time::now_iso8601;
use codecompass_core::types::generate_project_id;
use codecompass_core::vcs;
use codecompass_state::{db, import, maintenance_lock, schema};
use std::path::Path;

struct ImportRemapSpec<'a> {
    imported_project_id: &'a str,
    local_project_id: &'a str,
    workspace_str: &'a str,
    display_name: Option<String>,
    default_ref: &'a str,
    vcs_mode: i32,
    schema_version: u32,
    parser_version: u32,
}

pub fn run(workspace: &Path, bundle_path: &Path, config_file: Option<&Path>) -> Result<()> {
    let workspace = std::fs::canonicalize(workspace).context("Failed to resolve workspace path")?;
    let workspace_str = workspace.to_string_lossy().to_string();
    let config = Config::load_with_file(Some(&workspace), config_file)?;
    let project_id = generate_project_id(&workspace_str);
    let data_dir = config.project_data_dir(&project_id);
    let _maintenance_lock = maintenance_lock::acquire_project_lock(&data_dir, "state_import")?;

    let metadata = import::import_bundle(bundle_path, &data_dir)?;
    if metadata.parser_version != constants::PARSER_VERSION {
        eprintln!(
            "Warning: imported parser_version={} differs from local parser_version={}. Run `codecompass index --force --path {}` to refresh parser-dependent symbols.",
            metadata.parser_version,
            constants::PARSER_VERSION,
            workspace.display()
        );
    }
    let db_path = data_dir.join(constants::STATE_DB_FILE);
    let mut conn = db::open_connection(&db_path)?;
    schema::create_tables(&conn)?;

    let is_vcs_repo = vcs::is_git_repo(&workspace);
    let default_ref = if is_vcs_repo {
        vcs::detect_head_branch(&workspace).unwrap_or_else(|_| "main".to_string())
    } else {
        constants::REF_LIVE.to_string()
    };
    remap_imported_project_data(
        &mut conn,
        ImportRemapSpec {
            imported_project_id: &metadata.project_id,
            local_project_id: &project_id,
            workspace_str: &workspace_str,
            display_name: workspace
                .file_name()
                .map(|value| value.to_string_lossy().to_string()),
            default_ref: &default_ref,
            vcs_mode: i32::from(is_vcs_repo),
            schema_version: metadata.schema_version,
            parser_version: metadata.parser_version,
        },
    )?;

    conn.execute(
        "UPDATE branch_state SET status = 'stale' WHERE repo = ?1",
        rusqlite::params![project_id],
    )
    .map_err(codecompass_core::error::StateError::sqlite)?;

    println!("State import complete");
    println!("  Workspace: {}", workspace.display());
    println!("  Bundle: {}", bundle_path.display());
    println!("  Imported project ID: {}", metadata.project_id);
    println!(
        "  Local project ID: {}",
        generate_project_id(&workspace_str)
    );
    println!("  Schema version: {}", metadata.schema_version);
    Ok(())
}

fn remap_imported_project_data(
    conn: &mut rusqlite::Connection,
    spec: ImportRemapSpec<'_>,
) -> Result<()> {
    let tx = conn
        .transaction()
        .map_err(codecompass_core::error::StateError::sqlite)?;
    let created_at = existing_created_at(&tx, spec.local_project_id)?.unwrap_or_else(now_iso8601);
    let updated_at = now_iso8601();

    tx.execute(
        "INSERT INTO projects
         (project_id, repo_root, display_name, default_ref, vcs_mode, schema_version, parser_version, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(project_id) DO NOTHING",
        rusqlite::params![
            spec.local_project_id,
            spec.workspace_str,
            spec.display_name.clone(),
            spec.default_ref,
            spec.vcs_mode,
            spec.schema_version,
            spec.parser_version,
            created_at,
            updated_at,
        ],
    )
    .map_err(codecompass_core::error::StateError::sqlite)?;

    if spec.imported_project_id != spec.local_project_id {
        remap_repo_tables(&tx, spec.imported_project_id, spec.local_project_id)?;
        tx.execute(
            "UPDATE index_jobs SET project_id = ?1 WHERE project_id = ?2",
            rusqlite::params![spec.local_project_id, spec.imported_project_id],
        )
        .map_err(codecompass_core::error::StateError::sqlite)?;
        tx.execute(
            "UPDATE known_workspaces SET project_id = ?1 WHERE project_id = ?2",
            rusqlite::params![spec.local_project_id, spec.imported_project_id],
        )
        .map_err(codecompass_core::error::StateError::sqlite)?;
        tx.execute(
            "DELETE FROM projects WHERE project_id = ?1",
            rusqlite::params![spec.imported_project_id],
        )
        .map_err(codecompass_core::error::StateError::sqlite)?;
    }

    tx.execute(
        "INSERT INTO projects
         (project_id, repo_root, display_name, default_ref, vcs_mode, schema_version, parser_version, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(project_id) DO UPDATE SET
            repo_root = excluded.repo_root,
            display_name = excluded.display_name,
            default_ref = excluded.default_ref,
            vcs_mode = excluded.vcs_mode,
            schema_version = excluded.schema_version,
            parser_version = excluded.parser_version,
            updated_at = excluded.updated_at",
        rusqlite::params![
            spec.local_project_id,
            spec.workspace_str,
            spec.display_name,
            spec.default_ref,
            spec.vcs_mode,
            spec.schema_version,
            spec.parser_version,
            created_at,
            now_iso8601(),
        ],
    )
    .map_err(codecompass_core::error::StateError::sqlite)?;

    tx.commit()
        .map_err(codecompass_core::error::StateError::sqlite)?;
    Ok(())
}

fn existing_created_at(tx: &rusqlite::Transaction<'_>, project_id: &str) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;

    tx.query_row(
        "SELECT created_at FROM projects WHERE project_id = ?1",
        rusqlite::params![project_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(codecompass_core::error::StateError::sqlite)
    .map_err(Into::into)
}

fn remap_repo_tables(
    tx: &rusqlite::Transaction<'_>,
    imported_project_id: &str,
    local_project_id: &str,
) -> Result<()> {
    for table in [
        "file_manifest",
        "symbol_relations",
        "symbol_edges",
        "branch_state",
        "branch_tombstones",
        "worktree_leases",
    ] {
        let sql = format!("UPDATE {table} SET repo = ?1 WHERE repo = ?2");
        tx.execute(
            &sql,
            rusqlite::params![local_project_id, imported_project_id],
        )
        .map_err(codecompass_core::error::StateError::sqlite)?;
    }
    Ok(())
}
