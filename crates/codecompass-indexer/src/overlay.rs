use codecompass_core::error::StateError;
use codecompass_state::tantivy_index::IndexSet;
use std::path::{Path, PathBuf};

/// Returns `<data_dir>/overlay`.
pub fn overlay_root(data_dir: &Path) -> PathBuf {
    data_dir.join("overlay")
}

/// Normalize a ref name to a filesystem-safe overlay directory component.
///
/// Rules:
/// - `/` is converted to `-`
/// - `[A-Za-z0-9._-]` are preserved
/// - all other bytes are percent-encoded (`%XX`)
pub fn normalize_overlay_ref(ref_name: &str) -> String {
    let mut out = String::with_capacity(ref_name.len());
    for b in ref_name.bytes() {
        match b {
            b'/' => out.push('-'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    if out.is_empty() {
        "default".to_string()
    } else {
        out
    }
}

/// Returns `<data_dir>/overlay/<normalized_ref>`.
pub fn overlay_dir_for_ref(data_dir: &Path, ref_name: &str) -> PathBuf {
    overlay_root(data_dir).join(normalize_overlay_ref(ref_name))
}

/// Create overlay directory for a ref if missing.
pub fn create_overlay_dir(data_dir: &Path, ref_name: &str) -> Result<PathBuf, StateError> {
    let dir = overlay_dir_for_ref(data_dir, ref_name);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Create (or open) overlay Tantivy indices for a ref.
pub fn create_overlay_index_set(data_dir: &Path, ref_name: &str) -> Result<IndexSet, StateError> {
    let dir = create_overlay_dir(data_dir, ref_name)?;
    IndexSet::open_at(&dir)
}

/// List active overlay directories (normalized names).
pub fn list_active_overlays(data_dir: &Path) -> Result<Vec<String>, StateError> {
    let root = overlay_root(data_dir);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut overlays = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            overlays.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    overlays.sort();
    Ok(overlays)
}

/// Delete overlay directory for a ref if present.
pub fn delete_overlay_dir(data_dir: &Path, ref_name: &str) -> Result<(), StateError> {
    let dir = overlay_dir_for_ref(data_dir, ref_name);
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalize_overlay_ref_preserves_safe_and_encodes_unsafe() {
        assert_eq!(normalize_overlay_ref("main"), "main");
        assert_eq!(normalize_overlay_ref("feat/auth"), "feat-auth");
        assert_eq!(
            normalize_overlay_ref("feat/auth#2 with spaces"),
            "feat-auth%232%20with%20spaces"
        );
    }

    #[test]
    fn create_list_delete_overlay_dirs() {
        let dir = tempdir().unwrap();
        create_overlay_dir(dir.path(), "feat/auth").unwrap();
        create_overlay_dir(dir.path(), "fix/typo").unwrap();

        let overlays = list_active_overlays(dir.path()).unwrap();
        assert_eq!(
            overlays,
            vec!["feat-auth".to_string(), "fix-typo".to_string()]
        );

        delete_overlay_dir(dir.path(), "feat/auth").unwrap();
        let overlays = list_active_overlays(dir.path()).unwrap();
        assert_eq!(overlays, vec!["fix-typo".to_string()]);
    }

    #[test]
    fn create_overlay_index_set_creates_three_indices() {
        let dir = tempdir().unwrap();
        let _set = create_overlay_index_set(dir.path(), "feat/auth").unwrap();
        let overlay_dir = overlay_dir_for_ref(dir.path(), "feat/auth");
        assert!(overlay_dir.join("symbols").exists());
        assert!(overlay_dir.join("snippets").exists());
        assert!(overlay_dir.join("files").exists());
    }
}
