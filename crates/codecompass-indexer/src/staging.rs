use crate::overlay;
use codecompass_core::error::StateError;
use codecompass_state::tantivy_index::IndexSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayPublish {
    pub overlay_dir: PathBuf,
    pub backup_dir: Option<PathBuf>,
}

/// Returns `<data_dir>/staging`.
pub fn staging_root(data_dir: &Path) -> PathBuf {
    data_dir.join("staging")
}

/// Returns `<data_dir>/staging/<sync_id>`.
pub fn staging_dir(data_dir: &Path, sync_id: &str) -> PathBuf {
    staging_root(data_dir).join(sync_id)
}

/// Create (or open) staging index set for a sync.
pub fn create_staging_index_set(data_dir: &Path, sync_id: &str) -> Result<IndexSet, StateError> {
    let dir = staging_dir(data_dir, sync_id);
    std::fs::create_dir_all(&dir)?;
    IndexSet::open_at(&dir)
}

/// Atomically publish staging as overlay for `ref_name` via directory rename.
///
/// If an overlay already exists for this ref, it is moved to a backup path so
/// callers can restore it if subsequent metadata commit fails.
pub fn commit_staging_to_overlay(
    data_dir: &Path,
    sync_id: &str,
    ref_name: &str,
) -> Result<OverlayPublish, StateError> {
    let staging = staging_dir(data_dir, sync_id);
    if !staging.exists() {
        return Err(StateError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("staging directory not found for sync_id={sync_id}"),
        )));
    }

    let overlay_dir = overlay::overlay_dir_for_ref(data_dir, ref_name);
    if let Some(parent) = overlay_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut backup_dir = None;
    if overlay_dir.exists() {
        let name = overlay_dir
            .file_name()
            .map(|v| v.to_string_lossy().to_string())
            .unwrap_or_else(|| "overlay".to_string());
        let backup = overlay_dir.with_file_name(format!("{name}.bak.{sync_id}"));
        if backup.exists() {
            std::fs::remove_dir_all(&backup)?;
        }
        std::fs::rename(&overlay_dir, &backup)?;
        backup_dir = Some(backup);
    }

    if let Err(err) = std::fs::rename(&staging, &overlay_dir) {
        if let Some(backup) = backup_dir.as_ref() {
            let _ = std::fs::rename(backup, &overlay_dir);
        }
        return Err(StateError::Io(err));
    }

    Ok(OverlayPublish {
        overlay_dir,
        backup_dir,
    })
}

/// Finalize a successful publish by deleting old-overlay backup.
pub fn finalize_overlay_publish(publish: &OverlayPublish) -> Result<(), StateError> {
    if let Some(backup) = publish.backup_dir.as_ref()
        && backup.exists()
    {
        std::fs::remove_dir_all(backup)?;
    }
    Ok(())
}

/// Restore previous overlay after a failed post-publish metadata commit.
pub fn rollback_overlay_publish(publish: &OverlayPublish) -> Result<(), StateError> {
    if publish.overlay_dir.exists() {
        std::fs::remove_dir_all(&publish.overlay_dir)?;
    }
    if let Some(backup) = publish.backup_dir.as_ref()
        && backup.exists()
    {
        std::fs::rename(backup, &publish.overlay_dir)?;
    }
    Ok(())
}

/// Remove staging directory for `sync_id` (best-effort rollback).
pub fn rollback_staging(data_dir: &Path, sync_id: &str) -> Result<(), StateError> {
    let dir = staging_dir(data_dir, sync_id);
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Remove stale staging directories older than `older_than`.
///
/// Returns cleaned `sync_id` names.
pub fn cleanup_stale_staging(
    data_dir: &Path,
    older_than: SystemTime,
) -> Result<Vec<String>, StateError> {
    let root = staging_root(data_dir);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut removed = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let path = entry.path();
        let modified = entry
            .metadata()?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if modified <= older_than {
            std::fs::remove_dir_all(&path)?;
            removed.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    removed.sort();
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn staging_commit_moves_directory_to_overlay() {
        let dir = tempdir().unwrap();
        let sync_id = "sync-123";
        let ref_name = "feat/auth";

        let _set = create_staging_index_set(dir.path(), sync_id).unwrap();
        let staging = staging_dir(dir.path(), sync_id);
        assert!(staging.exists());

        let publish = commit_staging_to_overlay(dir.path(), sync_id, ref_name).unwrap();
        assert!(!staging.exists());
        assert!(publish.overlay_dir.exists());
        assert!(publish.backup_dir.is_none());
        assert!(publish.overlay_dir.join("symbols").exists());
        assert!(publish.overlay_dir.join("snippets").exists());
        assert!(publish.overlay_dir.join("files").exists());
    }

    #[test]
    fn staging_commit_with_existing_overlay_creates_backup_and_finalize_removes_it() {
        let dir = tempdir().unwrap();
        let ref_name = "feat/auth";
        let sync_id = "sync-backup";
        let overlay = overlay::create_overlay_dir(dir.path(), ref_name).unwrap();
        std::fs::write(overlay.join("old.marker"), "old").unwrap();

        let _set = create_staging_index_set(dir.path(), sync_id).unwrap();
        std::fs::write(staging_dir(dir.path(), sync_id).join("new.marker"), "new").unwrap();

        let publish = commit_staging_to_overlay(dir.path(), sync_id, ref_name).unwrap();
        assert!(publish.overlay_dir.join("new.marker").exists());
        let backup = publish.backup_dir.as_ref().expect("backup path");
        assert!(backup.join("old.marker").exists());

        finalize_overlay_publish(&publish).unwrap();
        assert!(!backup.exists());
    }

    #[test]
    fn rollback_overlay_publish_restores_previous_overlay() {
        let dir = tempdir().unwrap();
        let ref_name = "feat/auth";
        let sync_id = "sync-rollback-publish";
        let overlay = overlay::create_overlay_dir(dir.path(), ref_name).unwrap();
        std::fs::write(overlay.join("old.marker"), "old").unwrap();

        let _set = create_staging_index_set(dir.path(), sync_id).unwrap();
        std::fs::write(staging_dir(dir.path(), sync_id).join("new.marker"), "new").unwrap();

        let publish = commit_staging_to_overlay(dir.path(), sync_id, ref_name).unwrap();
        assert!(publish.overlay_dir.join("new.marker").exists());

        rollback_overlay_publish(&publish).unwrap();
        let restored = overlay::overlay_dir_for_ref(dir.path(), ref_name);
        assert!(restored.exists());
        assert!(restored.join("old.marker").exists());
        assert!(!restored.join("new.marker").exists());
    }

    #[test]
    fn rollback_removes_staging_directory() {
        let dir = tempdir().unwrap();
        let sync_id = "sync-rollback";
        let _set = create_staging_index_set(dir.path(), sync_id).unwrap();
        assert!(staging_dir(dir.path(), sync_id).exists());

        rollback_staging(dir.path(), sync_id).unwrap();
        assert!(!staging_dir(dir.path(), sync_id).exists());
    }

    #[test]
    fn cleanup_stale_staging_removes_older_dirs_only() {
        let dir = tempdir().unwrap();
        let old_id = "sync-old";
        let new_id = "sync-new";

        let _old = create_staging_index_set(dir.path(), old_id).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let cutoff = SystemTime::now();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _new = create_staging_index_set(dir.path(), new_id).unwrap();

        let removed = cleanup_stale_staging(dir.path(), cutoff).unwrap();
        assert_eq!(removed, vec![old_id.to_string()]);
        assert!(!staging_dir(dir.path(), old_id).exists());
        assert!(staging_dir(dir.path(), new_id).exists());
    }
}
