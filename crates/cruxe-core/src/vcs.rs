use crate::error::VcsError;
use std::path::Path;

/// Detect the current branch name from HEAD.
/// Returns the short branch name (e.g., "main", "feat/auth").
/// Returns an error if the repo cannot be opened or HEAD is detached.
pub fn detect_head_branch(repo_root: &Path) -> Result<String, VcsError> {
    let repo = git2::Repository::open(repo_root).map_err(|_| VcsError::NotGitRepo {
        path: repo_root.display().to_string(),
    })?;

    let head = repo
        .head()
        .map_err(|e| VcsError::GitError(format!("Failed to read HEAD: {}", e)))?;

    if let Some(shorthand) = head.shorthand() {
        Ok(shorthand.to_string())
    } else {
        Err(VcsError::GitError(
            "HEAD is detached or unnamed".to_string(),
        ))
    }
}

/// Check if a directory is a git repository.
pub fn is_git_repo(path: &Path) -> bool {
    git2::Repository::open(path).is_ok()
}

/// Get the current HEAD commit hash (short form, 12 characters).
pub fn detect_head_commit(repo_root: &Path) -> Result<String, VcsError> {
    let repo = git2::Repository::open(repo_root).map_err(|_| VcsError::NotGitRepo {
        path: repo_root.display().to_string(),
    })?;

    let head = repo
        .head()
        .map_err(|e| VcsError::GitError(format!("Failed to read HEAD: {}", e)))?;

    let commit = head
        .peel_to_commit()
        .map_err(|e| VcsError::GitError(format!("Failed to peel to commit: {}", e)))?;

    let oid = commit.id().to_string();
    Ok(oid[..12].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_git_repo_on_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_git_repo(dir.path()));
    }

    #[test]
    fn test_detect_head_branch_fails_on_non_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect_head_branch(dir.path()).is_err());
    }
}
