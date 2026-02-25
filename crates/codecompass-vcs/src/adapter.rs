use crate::diff::DiffEntry;
use codecompass_core::error::VcsError;
use std::path::Path;

pub trait VcsAdapter: Send + Sync {
    type FileChange;
    type DiffEntry;

    fn detect_repo(&self, repo_root: &Path) -> Result<(), VcsError>;
    fn resolve_head(&self, repo_root: &Path) -> Result<String, VcsError>;
    fn list_refs(&self, repo_root: &Path) -> Result<Vec<String>, VcsError>;
    fn merge_base(
        &self,
        repo_root: &Path,
        base_ref: &str,
        head_ref: &str,
    ) -> Result<String, VcsError>;
    fn diff_name_status(
        &self,
        repo_root: &Path,
        base_ref: &str,
        head_ref: &str,
    ) -> Result<Vec<Self::DiffEntry>, VcsError>;
    fn is_ancestor(
        &self,
        repo_root: &Path,
        ancestor: &str,
        descendant: &str,
    ) -> Result<bool, VcsError>;
    fn ensure_worktree(
        &self,
        repo_root: &Path,
        ref_name: &str,
        worktree_path: &Path,
    ) -> Result<(), VcsError>;
}

pub type DefaultDiffEntry = DiffEntry;
