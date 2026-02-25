use codecompass_core::error::StateError;
use std::path::{Path, PathBuf};

/// Best-effort canonical data directory normalization.
///
/// When the directory is missing (for example before first index),
/// returns the original path so callers can still construct deterministic
/// sibling locations.
pub fn canonicalize_data_dir(data_dir: &Path) -> PathBuf {
    std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf())
}

/// Canonicalize an overlay directory path.
pub fn canonicalize_overlay_dir(overlay_dir: &Path) -> Result<PathBuf, StateError> {
    std::fs::canonicalize(overlay_dir).map_err(StateError::Io)
}

/// Validate that a canonical overlay path is rooted under an allowed overlay directory.
///
/// Allowed roots:
/// - `<data_dir>/overlay`
/// - `<data_dir>/overlays`
pub fn is_overlay_dir_allowed(
    data_dir_canonical: &Path,
    overlay_canonical: &Path,
) -> Result<bool, StateError> {
    for allowed_root in allowed_overlay_roots(data_dir_canonical) {
        if !allowed_root.exists() {
            continue;
        }
        let root_canonical = std::fs::canonicalize(&allowed_root).map_err(StateError::Io)?;
        if overlay_canonical.starts_with(&root_canonical) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn allowed_overlay_roots(data_dir_canonical: &Path) -> [PathBuf; 2] {
    [
        data_dir_canonical.join("overlay"),
        data_dir_canonical.join("overlays"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_path_under_allowed_root_is_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        let overlay = data_dir.join("overlays").join("feat-auth");
        std::fs::create_dir_all(&overlay).unwrap();

        let data_dir_canonical = canonicalize_data_dir(&data_dir);
        let overlay_canonical = canonicalize_overlay_dir(&overlay).unwrap();
        assert!(is_overlay_dir_allowed(&data_dir_canonical, &overlay_canonical).unwrap());
    }

    #[test]
    fn overlay_path_outside_allowed_root_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(data_dir.join("overlays")).unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();

        let data_dir_canonical = canonicalize_data_dir(&data_dir);
        let outside_canonical = canonicalize_overlay_dir(&outside).unwrap();
        assert!(!is_overlay_dir_allowed(&data_dir_canonical, &outside_canonical).unwrap());
    }

    #[test]
    fn missing_allowed_roots_returns_false() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let candidate = tmp.path().join("candidate");
        std::fs::create_dir_all(&candidate).unwrap();

        let data_dir_canonical = canonicalize_data_dir(&data_dir);
        let candidate_canonical = canonicalize_overlay_dir(&candidate).unwrap();
        assert!(!is_overlay_dir_allowed(&data_dir_canonical, &candidate_canonical).unwrap());
    }
}
