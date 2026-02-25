use crate::error::VcsError;
use std::path::Path;

/// VCS abstraction boundary for 005 read/overlay operations.
///
/// Current default implementation delegates to git-backed helpers. Additional
/// adapters can implement this trait without changing MCP/tooling call sites.
pub trait VcsAdapter: Send + Sync {
    fn is_repo(&self, repo_root: &Path) -> bool;
    fn detect_head_branch(&self, repo_root: &Path) -> Result<String, VcsError>;
    fn detect_head_commit(&self, repo_root: &Path) -> Result<String, VcsError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct GitVcsAdapter;

impl VcsAdapter for GitVcsAdapter {
    fn is_repo(&self, repo_root: &Path) -> bool {
        crate::vcs::is_git_repo(repo_root)
    }

    fn detect_head_branch(&self, repo_root: &Path) -> Result<String, VcsError> {
        crate::vcs::detect_head_branch(repo_root)
    }

    fn detect_head_commit(&self, repo_root: &Path) -> Result<String, VcsError> {
        crate::vcs::detect_head_commit(repo_root)
    }
}

pub fn default_vcs_adapter() -> GitVcsAdapter {
    GitVcsAdapter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_adapter_reports_non_repo_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = default_vcs_adapter();
        assert!(!adapter.is_repo(dir.path()));
        assert!(adapter.detect_head_branch(dir.path()).is_err());
        assert!(adapter.detect_head_commit(dir.path()).is_err());
    }
}
