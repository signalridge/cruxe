use codecompass_core::error::StateError;
use codecompass_core::time::now_iso8601;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableStateMetadata {
    pub schema_version: u32,
    pub parser_version: u32,
    pub project_id: String,
    pub repo_root: String,
    pub exported_at: String,
}

impl PortableStateMetadata {
    pub fn new(
        schema_version: u32,
        parser_version: u32,
        project_id: impl Into<String>,
        repo_root: impl Into<String>,
    ) -> Self {
        Self {
            schema_version,
            parser_version,
            project_id: project_id.into(),
            repo_root: repo_root.into(),
            exported_at: now_iso8601(),
        }
    }
}

pub fn export_bundle(
    data_dir: &Path,
    bundle_path: &Path,
    metadata: &PortableStateMetadata,
) -> Result<(), StateError> {
    if !data_dir.exists() {
        return Err(StateError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("data directory does not exist: {}", data_dir.display()),
        )));
    }

    if let Some(parent) = bundle_path.parent() {
        std::fs::create_dir_all(parent).map_err(StateError::Io)?;
    }

    let file = File::create(bundle_path).map_err(StateError::Io)?;
    let encoder = zstd::Encoder::new(file, 3).map_err(StateError::Io)?;
    let mut tar = tar::Builder::new(encoder);

    let metadata_json = serde_json::to_vec_pretty(metadata)
        .map_err(|err| StateError::CorruptManifest(err.to_string()))?;
    append_bytes(&mut tar, "metadata.json", &metadata_json)?;

    for entry in std::fs::read_dir(data_dir).map_err(StateError::Io)? {
        let entry = entry.map_err(StateError::Io)?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            tar.append_dir_all(&name, &path).map_err(StateError::Io)?;
        } else if path.is_file() {
            tar.append_path_with_name(&path, &name)
                .map_err(StateError::Io)?;
        }
    }

    let encoder = tar.into_inner().map_err(StateError::Io)?;
    encoder.finish().map_err(StateError::Io)?;
    Ok(())
}

fn append_bytes<W: std::io::Write>(
    tar: &mut tar::Builder<W>,
    name: &str,
    payload: &[u8],
) -> Result<(), StateError> {
    let mut header = tar::Header::new_gnu();
    header.set_size(payload.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, name, payload)
        .map_err(StateError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bundle_writes_archive_with_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(data_dir.join("base")).unwrap();
        std::fs::write(data_dir.join("state.db"), b"sqlite-bytes").unwrap();
        std::fs::write(data_dir.join("base").join("marker.txt"), b"ok").unwrap();

        let out = tmp.path().join("bundle.tar.zst");
        let metadata = PortableStateMetadata::new(1, 1, "proj", "/tmp/repo");
        export_bundle(&data_dir, &out, &metadata).unwrap();
        assert!(out.exists());
    }
}
