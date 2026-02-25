use crate::export::PortableStateMetadata;
use codecompass_core::constants;
use codecompass_core::error::StateError;
use std::fs::File;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

/// Import a portable state bundle into `data_dir`.
///
/// Callers are expected to hold the project maintenance lock for the full
/// import lifecycle (bundle import + metadata remap + stale marking) to avoid
/// concurrent mutation races.
pub fn import_bundle(
    bundle_path: &Path,
    data_dir: &Path,
) -> Result<PortableStateMetadata, StateError> {
    if !bundle_path.exists() {
        return Err(StateError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("bundle not found: {}", bundle_path.display()),
        )));
    }

    let temp = tempfile::tempdir().map_err(StateError::Io)?;
    let file = File::open(bundle_path).map_err(StateError::Io)?;
    let decoder = zstd::Decoder::new(file).map_err(StateError::Io)?;
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(temp.path()).map_err(StateError::Io)?;

    let metadata_path = temp.path().join("metadata.json");
    if !metadata_path.exists() {
        return Err(StateError::CorruptManifest(
            "bundle is missing metadata.json".to_string(),
        ));
    }
    let metadata_bytes = std::fs::read(&metadata_path).map_err(StateError::Io)?;
    let metadata: PortableStateMetadata = serde_json::from_slice(&metadata_bytes)
        .map_err(|err| StateError::CorruptManifest(err.to_string()))?;

    if metadata.schema_version > constants::SCHEMA_VERSION {
        return Err(StateError::SchemaMigrationRequired {
            current: constants::SCHEMA_VERSION,
            required: metadata.schema_version,
        });
    }

    let parent_dir = data_dir.parent().ok_or_else(|| {
        StateError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("data directory has no parent: {}", data_dir.display()),
        ))
    })?;
    std::fs::create_dir_all(parent_dir).map_err(StateError::Io)?;
    let staged_data = tempfile::Builder::new()
        .prefix(".import-state-")
        .tempdir_in(parent_dir)
        .map_err(StateError::Io)?;

    for entry in std::fs::read_dir(temp.path()).map_err(StateError::Io)? {
        let entry = entry.map_err(StateError::Io)?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "metadata.json" {
            continue;
        }
        let destination = staged_data.path().join(&name);
        if path.is_dir() {
            copy_dir_recursive(&path, &destination)?;
        } else if path.is_file() {
            std::fs::copy(&path, &destination).map_err(StateError::Io)?;
        }
    }
    promote_staged_data(staged_data, data_dir)?;

    Ok(metadata)
}

fn promote_staged_data(staged_data: tempfile::TempDir, data_dir: &Path) -> Result<(), StateError> {
    let parent_dir = data_dir.parent().ok_or_else(|| {
        StateError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("data directory has no parent: {}", data_dir.display()),
        ))
    })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let backup_dir = parent_dir.join(format!(".state-backup-{stamp}"));
    let staged_path = staged_data.keep();
    let mut previous_moved = false;

    if data_dir.exists() {
        std::fs::rename(data_dir, &backup_dir).map_err(StateError::Io)?;
        previous_moved = true;
    }
    match std::fs::rename(&staged_path, data_dir) {
        Ok(()) => {
            if previous_moved
                && backup_dir.exists()
                && let Err(err) = std::fs::remove_dir_all(&backup_dir)
            {
                warn!(
                    backup_dir = %backup_dir.display(),
                    error = %err,
                    "Imported state committed, but failed to remove backup directory"
                );
            }
            Ok(())
        }
        Err(err) => {
            if previous_moved {
                let _ = std::fs::rename(&backup_dir, data_dir);
            }
            if staged_path.exists() {
                let _ = std::fs::remove_dir_all(&staged_path);
            }
            Err(StateError::Io(err))
        }
    }
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), StateError> {
    std::fs::create_dir_all(destination).map_err(StateError::Io)?;
    for entry in std::fs::read_dir(source).map_err(StateError::Io)? {
        let entry = entry.map_err(StateError::Io)?;
        let src_path = entry.path();
        let dst_path = destination.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(StateError::Io)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{PortableStateMetadata, export_bundle};

    #[test]
    fn import_bundle_restores_data_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let source_dir = tmp.path().join("source");
        std::fs::create_dir_all(source_dir.join("base")).unwrap();
        std::fs::write(source_dir.join("state.db"), b"sqlite").unwrap();
        std::fs::write(source_dir.join("base").join("marker.txt"), b"ok").unwrap();
        let bundle = tmp.path().join("bundle.tar.zst");
        export_bundle(
            &source_dir,
            &bundle,
            &PortableStateMetadata::new(1, 1, "proj", "/tmp/repo"),
        )
        .unwrap();

        let target_dir = tmp.path().join("target");
        let metadata = import_bundle(&bundle, &target_dir).unwrap();
        assert_eq!(metadata.project_id, "proj");
        assert!(target_dir.join("state.db").exists());
        assert!(target_dir.join("base").join("marker.txt").exists());
    }
}
