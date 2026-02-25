use codecompass_core::error::StateError;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const LOCK_DIR_NAME: &str = "locks";
const LOCK_FILE_PREFIX: &str = "state-maintenance-";
const LOCK_FILE_SUFFIX: &str = ".lock";

pub struct MaintenanceLock {
    file: File,
    path: PathBuf,
}

impl MaintenanceLock {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for MaintenanceLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn normalize_data_dir(data_dir: &Path) -> Result<PathBuf, StateError> {
    let absolute = if data_dir.is_absolute() {
        data_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(StateError::Io)?
            .join(data_dir)
    };
    if let Ok(canonical) = std::fs::canonicalize(&absolute) {
        return Ok(canonical);
    }

    let mut existing = absolute.as_path();
    let mut missing_components = Vec::new();
    loop {
        if existing.exists() {
            let mut normalized =
                std::fs::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
            for component in missing_components.iter().rev() {
                normalized.push(component);
            }
            return Ok(normalized);
        }
        let Some(name) = existing.file_name() else {
            return Ok(absolute);
        };
        missing_components.push(name.to_os_string());
        let Some(parent) = existing.parent() else {
            return Ok(absolute);
        };
        existing = parent;
    }
}

fn lock_path_from_normalized_data_dir(data_dir: &Path) -> Result<PathBuf, StateError> {
    let Some(anchor_dir) = data_dir.parent() else {
        return Err(StateError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("data directory has no parent: {}", data_dir.display()),
        )));
    };
    let path_hash = blake3::hash(data_dir.to_string_lossy().as_bytes());
    let lock_file_name = format!(
        "{LOCK_FILE_PREFIX}{}{LOCK_FILE_SUFFIX}",
        &path_hash.to_hex()[..16]
    );
    Ok(anchor_dir.join(LOCK_DIR_NAME).join(lock_file_name))
}

pub fn project_lock_path(data_dir: &Path) -> Result<PathBuf, StateError> {
    let normalized_data_dir = normalize_data_dir(data_dir)?;
    lock_path_from_normalized_data_dir(&normalized_data_dir)
}

pub fn acquire_project_lock(
    data_dir: &Path,
    operation: &str,
) -> Result<MaintenanceLock, StateError> {
    let lock_path = project_lock_path(data_dir)?;
    if let Some(lock_dir) = lock_path.parent() {
        std::fs::create_dir_all(lock_dir).map_err(StateError::Io)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(StateError::Io)?;

    if let Err(err) = file.try_lock_exclusive() {
        if err.kind() == std::io::ErrorKind::WouldBlock {
            return Err(StateError::maintenance_lock_busy(
                operation,
                lock_path.display().to_string(),
            ));
        }
        return Err(StateError::Io(err));
    }

    let _ = file.set_len(0);
    let _ = writeln!(file, "operation={operation}");
    let _ = writeln!(file, "pid={}", std::process::id());
    let _ = writeln!(file, "data_dir={}", data_dir.display());
    let _ = writeln!(file, "timestamp={}", codecompass_core::time::now_iso8601());
    let _ = file.sync_data();

    Ok(MaintenanceLock {
        file,
        path: lock_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_lock_creates_lock_file_outside_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("state").join("project-a");
        std::fs::create_dir_all(&data_dir).unwrap();
        let lock = acquire_project_lock(&data_dir, "test_lock").unwrap();

        assert!(lock.path().exists());
        assert!(!lock.path().starts_with(&data_dir));
        let expected_lock_root =
            std::fs::canonicalize(data_dir.parent().unwrap().join(LOCK_DIR_NAME)).unwrap();
        assert!(lock.path().starts_with(&expected_lock_root));

        let content = std::fs::read_to_string(lock.path()).unwrap();
        assert!(content.contains("operation=test_lock"));
    }

    #[test]
    fn lock_can_be_reacquired_after_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("state");
        {
            let _lock = acquire_project_lock(&data_dir, "first").unwrap();
        }
        let second = acquire_project_lock(&data_dir, "second").unwrap();
        let content = std::fs::read_to_string(second.path()).unwrap();
        assert!(content.contains("operation=second"));
    }

    #[test]
    fn lock_path_stays_stable_across_data_dir_swap() {
        let tmp = tempfile::tempdir().unwrap();
        let data_root = tmp.path().join("state");
        let data_dir = data_root.join("project-a");
        let staged_dir = data_root.join("staged-project");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&staged_dir).unwrap();

        let before = project_lock_path(&data_dir).unwrap();
        let backup_dir = data_root.join("project-a-backup");
        std::fs::rename(&data_dir, &backup_dir).unwrap();
        std::fs::rename(&staged_dir, &data_dir).unwrap();

        let after = project_lock_path(&data_dir).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn lock_path_stays_stable_before_and_after_data_dir_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("state").join("project-a");

        let before_create = project_lock_path(&data_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let after_create = project_lock_path(&data_dir).unwrap();

        assert_eq!(before_create, after_create);
    }
}
