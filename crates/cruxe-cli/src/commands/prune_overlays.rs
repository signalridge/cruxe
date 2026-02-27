use anyhow::{Context, Result, bail};
use cruxe_core::config::Config;
use cruxe_core::constants;
use cruxe_core::types::generate_project_id;
use cruxe_indexer::overlay;
use cruxe_state::{
    branch_state, db, maintenance_lock, overlay_paths, project, schema, worktree_leases,
};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub fn run(workspace: &Path, older_than_days: u64, config_file: Option<&Path>) -> Result<()> {
    let workspace = std::fs::canonicalize(workspace).context("Failed to resolve workspace path")?;
    let workspace_str = workspace.to_string_lossy().to_string();
    let config = Config::load_with_file(Some(&workspace), config_file)?;
    let project_id = generate_project_id(&workspace_str);
    let data_dir = config.project_data_dir(&project_id);
    let _maintenance_lock = maintenance_lock::acquire_project_lock(&data_dir, "prune_overlays")?;
    let db_path = data_dir.join(constants::STATE_DB_FILE);
    let conn = db::open_connection(&db_path)?;
    schema::create_tables(&conn)?;

    let Some(_project_row) = project::get_by_root(&conn, &workspace_str)? else {
        bail!(
            "Project not initialized. Run `cruxe init --path {}` first.",
            workspace_str
        );
    };

    let retention_days = i64::try_from(older_than_days)
        .context("--older-than is too large for timestamp arithmetic")?;
    let cutoff = OffsetDateTime::now_utc() - time::Duration::days(retention_days);
    let data_dir_canonical = overlay_paths::canonicalize_data_dir(&data_dir);
    let mut removed = 0usize;
    let mut kept_active = 0usize;
    let mut kept_recent = 0usize;
    let mut kept_unsafe = 0usize;
    let mut kept_remove_error = 0usize;

    for branch in branch_state::list_branch_states(&conn, &project_id)? {
        if branch.is_default_branch {
            continue;
        }
        let Some(last_accessed) = parse_timestamp(&branch.last_accessed_at) else {
            kept_recent += 1;
            continue;
        };
        if last_accessed > cutoff {
            kept_recent += 1;
            continue;
        }
        if let Some(lease) = worktree_leases::get_lease(&conn, &project_id, &branch.r#ref)?
            && lease.status == "active"
            && lease.refcount > 0
        {
            kept_active += 1;
            continue;
        }

        let overlay_dir =
            resolve_overlay_dir(&data_dir, &branch.r#ref, branch.overlay_dir.as_deref());
        let Some(safe_overlay_dir) = validate_prune_target(&data_dir_canonical, &overlay_dir)?
        else {
            kept_unsafe += 1;
            continue;
        };
        if safe_overlay_dir.exists()
            && let Err(err) = std::fs::remove_dir_all(&safe_overlay_dir)
                .with_context(|| format!("failed to remove {}", safe_overlay_dir.display()))
        {
            eprintln!("Warning: {err}");
            kept_remove_error += 1;
            continue;
        }
        branch_state::set_status(&conn, &project_id, &branch.r#ref, "evicted")?;
        removed += 1;
    }

    println!("Overlay prune complete");
    println!("  Workspace: {}", workspace.display());
    println!("  Removed overlays: {}", removed);
    println!("  Kept (active lease): {}", kept_active);
    println!("  Kept (recent): {}", kept_recent);
    println!("  Kept (unsafe path): {}", kept_unsafe);
    println!("  Kept (remove error): {}", kept_remove_error);
    Ok(())
}

fn resolve_overlay_dir(data_dir: &Path, ref_name: &str, overlay_dir: Option<&str>) -> PathBuf {
    overlay_dir
        .map(Path::new)
        .map(|path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                data_dir.join(path)
            }
        })
        .unwrap_or_else(|| overlay::overlay_dir_for_ref(data_dir, ref_name))
}

fn validate_prune_target(data_dir_canonical: &Path, overlay_dir: &Path) -> Result<Option<PathBuf>> {
    if !overlay_dir.exists() {
        return Ok(Some(overlay_dir.to_path_buf()));
    }
    let overlay_canonical = overlay_paths::canonicalize_overlay_dir(overlay_dir)
        .with_context(|| format!("failed to resolve {}", overlay_dir.display()))?;
    if overlay_paths::is_overlay_dir_allowed(data_dir_canonical, &overlay_canonical).with_context(
        || {
            format!(
                "failed to validate overlay safety roots under {}",
                data_dir_canonical.display()
            )
        },
    )? {
        return Ok(Some(overlay_canonical));
    }
    Ok(None)
}

fn parse_timestamp(value: &str) -> Option<OffsetDateTime> {
    if let Ok(parsed) = OffsetDateTime::parse(value, &Rfc3339) {
        return Some(parsed);
    }
    let legacy =
        time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]").ok()?;
    OffsetDateTime::parse(value, &legacy).ok()
}
