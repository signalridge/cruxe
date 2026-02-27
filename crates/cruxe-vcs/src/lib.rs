pub mod adapter;
pub mod diff;
pub mod git2_adapter;
pub mod worktree;

pub use adapter::VcsAdapter;
pub use diff::{DiffEntry, FileChangeKind};
pub use git2_adapter::Git2VcsAdapter;
pub use worktree::{WorktreeManager, normalize_ref_name};

#[cfg(test)]
mod tests {
    use super::{Git2VcsAdapter, VcsAdapter};

    #[test]
    fn crate_exports_are_usable() {
        let adapter = Git2VcsAdapter;
        let temp = tempfile::tempdir().unwrap();
        assert!(adapter.detect_repo(temp.path()).is_err());
    }
}
