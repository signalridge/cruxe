use crate::adapter::VcsAdapter;
use crate::diff::{DiffEntry, FileChangeKind};
use crate::worktree::normalize_ref_name;
use codecompass_core::error::VcsError;
use git2::{DiffFindOptions, DiffOptions, Oid, Repository, WorktreeAddOptions};
use std::path::Path;

#[derive(Debug, Default, Clone, Copy)]
pub struct Git2VcsAdapter;

impl Git2VcsAdapter {
    fn open_repo(repo_root: &Path) -> Result<Repository, VcsError> {
        Repository::open(repo_root).map_err(|_| VcsError::NotGitRepo {
            path: repo_root.display().to_string(),
        })
    }

    fn rev_to_oid(repo: &Repository, rev: &str) -> Result<Oid, VcsError> {
        repo.revparse_single(rev)
            .map(|obj| obj.id())
            .map_err(|e| VcsError::GitError(format!("failed to resolve revision `{rev}`: {e}")))
    }

    fn short_ref_name(ref_name: &str) -> &str {
        ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name)
    }
}

impl VcsAdapter for Git2VcsAdapter {
    type FileChange = FileChangeKind;
    type DiffEntry = DiffEntry;

    fn detect_repo(&self, repo_root: &Path) -> Result<(), VcsError> {
        Self::open_repo(repo_root).map(|_| ())
    }

    fn resolve_head(&self, repo_root: &Path) -> Result<String, VcsError> {
        let repo = Self::open_repo(repo_root)?;
        let head = repo
            .head()
            .map_err(|e| VcsError::GitError(format!("failed to read HEAD: {e}")))?;
        let commit = head
            .peel_to_commit()
            .map_err(|e| VcsError::GitError(format!("failed to resolve HEAD commit: {e}")))?;
        Ok(commit.id().to_string())
    }

    fn list_refs(&self, repo_root: &Path) -> Result<Vec<String>, VcsError> {
        let repo = Self::open_repo(repo_root)?;
        let mut refs = Vec::new();

        let branches = repo
            .branches(None)
            .map_err(|e| VcsError::GitError(format!("failed to enumerate branches: {e}")))?;
        for branch in branches {
            let (branch, _) =
                branch.map_err(|e| VcsError::GitError(format!("failed to read branch: {e}")))?;
            if let Some(name) = branch
                .name()
                .map_err(|e| VcsError::GitError(format!("failed to get branch name: {e}")))?
            {
                refs.push(name.to_string());
            }
        }

        refs.sort();
        refs.dedup();
        Ok(refs)
    }

    fn merge_base(
        &self,
        repo_root: &Path,
        base_ref: &str,
        head_ref: &str,
    ) -> Result<String, VcsError> {
        let repo = Self::open_repo(repo_root)?;
        let base = Self::rev_to_oid(&repo, base_ref)?;
        let head = Self::rev_to_oid(&repo, head_ref)?;
        let merge_base = repo
            .merge_base(base, head)
            .map_err(|e| VcsError::GitError(format!("failed to compute merge base: {e}")))?;
        Ok(merge_base.to_string())
    }

    fn diff_name_status(
        &self,
        repo_root: &Path,
        base_ref: &str,
        head_ref: &str,
    ) -> Result<Vec<Self::DiffEntry>, VcsError> {
        let repo = Self::open_repo(repo_root)?;
        let base_obj = repo.revparse_single(base_ref).map_err(|e| {
            VcsError::GitError(format!("failed to resolve base ref `{base_ref}`: {e}"))
        })?;
        let head_obj = repo.revparse_single(head_ref).map_err(|e| {
            VcsError::GitError(format!("failed to resolve head ref `{head_ref}`: {e}"))
        })?;

        let base_commit = base_obj
            .peel_to_commit()
            .map_err(|e| VcsError::GitError(format!("failed to peel base commit: {e}")))?;
        let head_commit = head_obj
            .peel_to_commit()
            .map_err(|e| VcsError::GitError(format!("failed to peel head commit: {e}")))?;

        let base_tree = base_commit
            .tree()
            .map_err(|e| VcsError::GitError(format!("failed to load base tree: {e}")))?;
        let head_tree = head_commit
            .tree()
            .map_err(|e| VcsError::GitError(format!("failed to load head tree: {e}")))?;

        let mut diff_opts = DiffOptions::new();
        diff_opts.include_typechange(true).include_untracked(false);
        let mut diff = repo
            .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut diff_opts))
            .map_err(|e| VcsError::GitError(format!("failed to compute diff: {e}")))?;
        let mut find_opts = DiffFindOptions::new();
        find_opts.renames(true);
        diff.find_similar(Some(&mut find_opts))
            .map_err(|e| VcsError::GitError(format!("failed to detect renames: {e}")))?;

        let mut out = Vec::new();
        for delta in diff.deltas() {
            let old_path = delta
                .old_file()
                .path()
                .map(|p| p.to_string_lossy().to_string());
            let new_path = delta
                .new_file()
                .path()
                .map(|p| p.to_string_lossy().to_string());

            match delta.status() {
                git2::Delta::Added => {
                    if let Some(path) = new_path {
                        out.push(DiffEntry::added(path));
                    }
                }
                git2::Delta::Deleted => {
                    if let Some(path) = old_path {
                        out.push(DiffEntry::deleted(path));
                    }
                }
                git2::Delta::Renamed => {
                    if let (Some(old_path), Some(new_path)) = (old_path, new_path) {
                        out.push(DiffEntry::renamed(old_path, new_path));
                    }
                }
                _ => {
                    if let Some(path) = new_path.or(old_path) {
                        out.push(DiffEntry::modified(path));
                    }
                }
            }
        }
        Ok(out)
    }

    fn is_ancestor(
        &self,
        repo_root: &Path,
        ancestor: &str,
        descendant: &str,
    ) -> Result<bool, VcsError> {
        let repo = Self::open_repo(repo_root)?;
        let ancestor_oid = Self::rev_to_oid(&repo, ancestor)?;
        let descendant_oid = Self::rev_to_oid(&repo, descendant)?;
        repo.graph_descendant_of(descendant_oid, ancestor_oid)
            .map_err(|e| VcsError::GitError(format!("failed to evaluate ancestry: {e}")))
    }

    fn ensure_worktree(
        &self,
        repo_root: &Path,
        ref_name: &str,
        worktree_path: &Path,
    ) -> Result<(), VcsError> {
        if worktree_path.join(".git").exists() {
            return Ok(());
        }

        let repo = Self::open_repo(repo_root)?;
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                VcsError::GitError(format!("failed to create worktree parent: {e}"))
            })?;
        }

        if let Ok(head) = repo.head()
            && head.shorthand() == Some(Self::short_ref_name(ref_name))
        {
            // Requested ref is already checked out in this repository root.
            return Ok(());
        }

        let name = normalize_ref_name(ref_name);
        let full_ref = if ref_name.starts_with("refs/") {
            ref_name.to_string()
        } else {
            format!("refs/heads/{ref_name}")
        };
        let reference = repo.find_reference(&full_ref).map_err(|e| {
            VcsError::GitError(format!("failed to resolve worktree ref `{full_ref}`: {e}"))
        })?;
        let mut opts = WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(&name, worktree_path, Some(&opts))
            .map_err(|e| VcsError::GitError(format!("failed to create worktree: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_git_repo(dir: &std::path::Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        let file = dir.join("src.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("src.rs")).unwrap();
        let tree_id = index.write_tree().unwrap();
        {
            let tree = repo.find_tree(tree_id).unwrap();
            let sig = git2::Signature::now("test", "test@example.com").unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();
        }
        repo
    }

    #[test]
    fn adapter_core_operations_work_on_basic_repo() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_git_repo(dir.path());
        let adapter = Git2VcsAdapter;

        assert!(adapter.detect_repo(dir.path()).is_ok());
        let head = adapter.resolve_head(dir.path()).unwrap();
        assert!(!head.is_empty());

        let refs = adapter.list_refs(dir.path()).unwrap();
        assert!(!refs.is_empty());
    }

    #[test]
    fn merge_base_and_is_ancestor_are_consistent() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_git_repo(dir.path());
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feat/auth", &head_commit, false).unwrap();

        std::fs::write(dir.path().join("src.rs"), "fn auth() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("src.rs")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(
            Some("refs/heads/feat/auth"),
            &sig,
            &sig,
            "feat",
            &tree,
            &[&head_commit],
        )
        .unwrap();

        let adapter = Git2VcsAdapter;
        let mb = adapter
            .merge_base(dir.path(), "HEAD", "refs/heads/feat/auth")
            .unwrap();
        assert_eq!(mb, head_commit.id().to_string());
        let head = repo
            .find_reference("refs/heads/feat/auth")
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .id()
            .to_string();
        assert!(adapter.is_ancestor(dir.path(), &mb, &head).unwrap());
    }

    #[test]
    fn diff_name_status_returns_modified_entries() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_git_repo(dir.path());
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        let base_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feat/diff", &base_commit, false).unwrap();
        std::fs::write(dir.path().join("src.rs"), "fn changed() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("src.rs")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(
            Some("refs/heads/feat/diff"),
            &sig,
            &sig,
            "change",
            &tree,
            &[&base_commit],
        )
        .unwrap();

        let adapter = Git2VcsAdapter;
        let diff = adapter
            .diff_name_status(dir.path(), "HEAD", "refs/heads/feat/diff")
            .unwrap();
        assert!(diff.iter().any(|d| d.path == "src.rs"));
    }

    #[test]
    fn ensure_worktree_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_git_repo(dir.path());
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feat/worktree", &head, false).unwrap();
        std::fs::write(dir.path().join("feature.rs"), "fn feature() {}\n").unwrap();
        let mut idx = repo.index().unwrap();
        idx.add_path(std::path::Path::new("feature.rs")).unwrap();
        let tree_id = idx.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        repo.commit(
            Some("refs/heads/feat/worktree"),
            &sig,
            &sig,
            "feature commit",
            &tree,
            &[&head],
        )
        .unwrap();

        let adapter = Git2VcsAdapter;
        let worktree_path = dir.path().join("worktrees/feat-worktree");
        adapter
            .ensure_worktree(dir.path(), "feat/worktree", &worktree_path)
            .unwrap();
        assert!(worktree_path.exists());
        assert!(
            worktree_path.join("feature.rs").exists(),
            "worktree should be checked out at requested ref"
        );
    }
}
