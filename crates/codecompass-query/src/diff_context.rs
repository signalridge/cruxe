use codecompass_core::error::StateError;
use codecompass_state::branch_state;
use codecompass_vcs::{DiffEntry, FileChangeKind, Git2VcsAdapter, VcsAdapter};
use rusqlite::{Connection, params_from_iter};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffLineRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffSymbolSnapshot {
    pub symbol_id: String,
    pub symbol_stable_id: String,
    pub kind: String,
    pub qualified_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffFileChange {
    pub path: String,
    pub change_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffSymbolChange {
    pub symbol: String,
    pub change_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<DiffSymbolSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<DiffSymbolSnapshot>,
    pub path: String,
    pub lines: DiffLineRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffContextResult {
    pub base_ref: String,
    pub head_ref: String,
    pub merge_base_commit: String,
    pub affected_files: usize,
    pub file_changes: Vec<DiffFileChange>,
    pub changes: Vec<DiffSymbolChange>,
}

#[derive(Debug, Clone)]
struct SymbolWithHash {
    symbol: DiffSymbolSnapshot,
    content_hash: String,
    name: String,
}

pub fn diff_context(
    conn: &Connection,
    repo_root: &Path,
    project_id: &str,
    base_ref: &str,
    head_ref: &str,
    path_filter: Option<&str>,
    limit: usize,
) -> Result<DiffContextResult, StateError> {
    ensure_ref_indexed(conn, project_id, base_ref)?;
    ensure_ref_indexed(conn, project_id, head_ref)?;

    let adapter = Git2VcsAdapter;
    let merge_base_commit = adapter.merge_base(repo_root, base_ref, head_ref).map_err(
        |e: codecompass_core::error::VcsError| {
            StateError::merge_base_failed(base_ref, head_ref, e.to_string())
        },
    )?;
    let mut diff_entries = adapter
        .diff_name_status(repo_root, &merge_base_commit, head_ref)
        .map_err(StateError::vcs)?;

    if let Some(prefix) = path_filter.filter(|value| !value.trim().is_empty()) {
        diff_entries.retain(|entry| matches_path_filter(entry, prefix));
    }

    let affected_files = diff_entries.len();
    let file_changes = summarize_file_changes(&diff_entries);

    let changed_paths = collect_changed_paths(&diff_entries);
    let base_symbols = load_symbols_for_paths(conn, project_id, base_ref, &changed_paths)?;
    let head_symbols = load_symbols_for_paths(conn, project_id, head_ref, &changed_paths)?;

    let mut keys = BTreeSet::new();
    keys.extend(base_symbols.keys().cloned());
    keys.extend(head_symbols.keys().cloned());

    let mut changes = Vec::new();
    for key in keys {
        let before = base_symbols.get(&key);
        let after = head_symbols.get(&key);
        let Some(change) = classify_symbol_change(before, after) else {
            continue;
        };
        changes.push(change);
    }

    changes.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.lines.start.cmp(&b.lines.start))
            .then_with(|| a.symbol.cmp(&b.symbol))
    });

    if limit > 0 && changes.len() > limit {
        changes.truncate(limit);
    }

    Ok(DiffContextResult {
        base_ref: base_ref.to_string(),
        head_ref: head_ref.to_string(),
        merge_base_commit,
        affected_files,
        file_changes,
        changes,
    })
}

fn ensure_ref_indexed(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
) -> Result<(), StateError> {
    if branch_state::get_branch_state(conn, project_id, ref_name)?.is_none() {
        return Err(StateError::ref_not_indexed(project_id, ref_name));
    }
    Ok(())
}

fn matches_path_filter(entry: &DiffEntry, prefix: &str) -> bool {
    if entry.path.starts_with(prefix) {
        return true;
    }
    match &entry.kind {
        FileChangeKind::Renamed { old_path } => old_path.starts_with(prefix),
        _ => false,
    }
}

fn summarize_file_changes(entries: &[DiffEntry]) -> Vec<DiffFileChange> {
    entries
        .iter()
        .map(|entry| match &entry.kind {
            FileChangeKind::Added => DiffFileChange {
                path: entry.path.clone(),
                change_type: "added".to_string(),
                old_path: None,
            },
            FileChangeKind::Modified => DiffFileChange {
                path: entry.path.clone(),
                change_type: "modified".to_string(),
                old_path: None,
            },
            FileChangeKind::Deleted => DiffFileChange {
                path: entry.path.clone(),
                change_type: "deleted".to_string(),
                old_path: None,
            },
            FileChangeKind::Renamed { old_path } => DiffFileChange {
                path: entry.path.clone(),
                change_type: "renamed".to_string(),
                old_path: Some(old_path.clone()),
            },
        })
        .collect()
}

fn collect_changed_paths(entries: &[DiffEntry]) -> HashSet<String> {
    let mut paths = HashSet::new();
    for entry in entries {
        paths.insert(entry.path.clone());
        if let FileChangeKind::Renamed { old_path } = &entry.kind {
            paths.insert(old_path.clone());
        }
    }
    paths
}

fn load_symbols_for_paths(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    paths: &HashSet<String>,
) -> Result<HashMap<(String, String), SymbolWithHash>, StateError> {
    if paths.is_empty() {
        return Ok(HashMap::new());
    }

    let mut path_list: Vec<&String> = paths.iter().collect();
    path_list.sort();
    let mut result = HashMap::new();
    // SQLite bind parameter limits vary by build; keep a conservative chunk size.
    const SQLITE_PARAM_LIMIT: usize = 999;
    const FIXED_PARAMS: usize = 2; // repo + ref
    let chunk_size = SQLITE_PARAM_LIMIT.saturating_sub(FIXED_PARAMS).max(1);
    for chunk in path_list.chunks(chunk_size) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let query = format!(
            "SELECT symbol_id, symbol_stable_id, kind, qualified_name, signature, path, \
                    line_start, line_end, content_hash, name \
             FROM symbol_relations \
             WHERE repo = ? AND \"ref\" = ? AND path IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&query).map_err(StateError::sqlite)?;

        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() + FIXED_PARAMS);
        params.push(&repo);
        params.push(&ref_name);
        for path in chunk {
            params.push(*path);
        }

        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                let snapshot = DiffSymbolSnapshot {
                    symbol_id: row.get(0)?,
                    symbol_stable_id: row.get(1)?,
                    kind: row.get(2)?,
                    qualified_name: row.get(3)?,
                    signature: row.get(4)?,
                    path: row.get(5)?,
                    line_start: row.get(6)?,
                    line_end: row.get(7)?,
                };
                let content_hash: String = row.get(8)?;
                let name: String = row.get(9)?;
                Ok(SymbolWithHash {
                    symbol: snapshot,
                    content_hash,
                    name,
                })
            })
            .map_err(StateError::sqlite)?;

        for row in rows {
            let symbol = row.map_err(StateError::sqlite)?;
            result.insert(
                (
                    symbol.symbol.symbol_stable_id.clone(),
                    symbol.symbol.kind.clone(),
                ),
                symbol,
            );
        }
    }
    Ok(result)
}

fn classify_symbol_change(
    before: Option<&SymbolWithHash>,
    after: Option<&SymbolWithHash>,
) -> Option<DiffSymbolChange> {
    match (before, after) {
        (None, Some(current)) => Some(DiffSymbolChange {
            symbol: current.name.clone(),
            change_type: "added".to_string(),
            before: None,
            after: Some(current.symbol.clone()),
            path: current.symbol.path.clone(),
            lines: DiffLineRange {
                start: current.symbol.line_start,
                end: current.symbol.line_end,
            },
        }),
        (Some(previous), None) => Some(DiffSymbolChange {
            symbol: previous.name.clone(),
            change_type: "deleted".to_string(),
            before: Some(previous.symbol.clone()),
            after: None,
            path: previous.symbol.path.clone(),
            lines: DiffLineRange {
                start: previous.symbol.line_start,
                end: previous.symbol.line_end,
            },
        }),
        (Some(previous), Some(current)) => {
            let changed = previous.content_hash != current.content_hash
                || previous.symbol.signature != current.symbol.signature;
            if !changed {
                return None;
            }
            Some(DiffSymbolChange {
                symbol: current.name.clone(),
                change_type: "modified".to_string(),
                before: Some(previous.symbol.clone()),
                after: Some(current.symbol.clone()),
                path: current.symbol.path.clone(),
                lines: DiffLineRange {
                    start: current.symbol.line_start,
                    end: current.symbol.line_end,
                },
            })
        }
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codecompass_core::types::{SymbolKind, SymbolRecord};
    use codecompass_state::{db, schema, symbols};

    fn init_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn insert_symbol(
        conn: &Connection,
        repo: &str,
        ref_name: &str,
        path: &str,
        stable_id: &str,
        symbol_id: &str,
        name: &str,
        signature: &str,
        content: &str,
    ) {
        symbols::insert_symbol(
            conn,
            &SymbolRecord {
                repo: repo.to_string(),
                r#ref: ref_name.to_string(),
                commit: None,
                path: path.to_string(),
                language: "rust".to_string(),
                symbol_id: symbol_id.to_string(),
                symbol_stable_id: stable_id.to_string(),
                name: name.to_string(),
                qualified_name: format!("crate::{name}"),
                kind: SymbolKind::Function,
                signature: Some(signature.to_string()),
                line_start: 1,
                line_end: 3,
                parent_symbol_id: None,
                visibility: Some("pub".to_string()),
                content: Some(content.to_string()),
            },
        )
        .unwrap();
    }

    #[test]
    fn diff_context_classifies_added_modified_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);

        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn keep() {}\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "base"]);

        git(&repo, &["checkout", "-b", "feat/diff"]);
        std::fs::write(
            repo.join("src/lib.rs"),
            "pub fn keep() {}\npub fn modified(v: i32) -> i32 { v }\n",
        )
        .unwrap();
        std::fs::write(repo.join("src/new.rs"), "pub fn added() {}\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "feature"]);

        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        branch_state::upsert_branch_state(
            &conn,
            &branch_state::BranchState {
                repo: "proj".to_string(),
                r#ref: "main".to_string(),
                merge_base_commit: None,
                last_indexed_commit: "a".to_string(),
                overlay_dir: None,
                file_count: 1,
                symbol_count: 3,
                is_default_branch: true,
                status: "active".to_string(),
                eviction_eligible_at: None,
                created_at: "2026-02-25T00:00:00Z".to_string(),
                last_accessed_at: "2026-02-25T00:00:00Z".to_string(),
            },
        )
        .unwrap();
        branch_state::upsert_branch_state(
            &conn,
            &branch_state::BranchState {
                repo: "proj".to_string(),
                r#ref: "feat/diff".to_string(),
                merge_base_commit: None,
                last_indexed_commit: "b".to_string(),
                overlay_dir: None,
                file_count: 2,
                symbol_count: 3,
                is_default_branch: false,
                status: "active".to_string(),
                eviction_eligible_at: None,
                created_at: "2026-02-25T00:00:00Z".to_string(),
                last_accessed_at: "2026-02-25T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        insert_symbol(
            &conn,
            "proj",
            "main",
            "src/lib.rs",
            "stable-keep",
            "sym-keep-main",
            "keep",
            "pub fn keep() {}",
            "keep main body",
        );
        insert_symbol(
            &conn,
            "proj",
            "main",
            "src/lib.rs",
            "stable-modified",
            "sym-mod-main",
            "modified",
            "pub fn modified() {}",
            "before",
        );
        insert_symbol(
            &conn,
            "proj",
            "main",
            "src/lib.rs",
            "stable-deleted",
            "sym-del-main",
            "deleted",
            "pub fn deleted() {}",
            "deleted body",
        );

        insert_symbol(
            &conn,
            "proj",
            "feat/diff",
            "src/lib.rs",
            "stable-keep",
            "sym-keep-head",
            "keep",
            "pub fn keep() {}",
            "keep main body",
        );
        insert_symbol(
            &conn,
            "proj",
            "feat/diff",
            "src/lib.rs",
            "stable-modified",
            "sym-mod-head",
            "modified",
            "pub fn modified(v: i32) -> i32",
            "after",
        );
        insert_symbol(
            &conn,
            "proj",
            "feat/diff",
            "src/new.rs",
            "stable-added",
            "sym-add-head",
            "added",
            "pub fn added() {}",
            "added body",
        );

        let diff = diff_context(&conn, &repo, "proj", "main", "feat/diff", None, 50).unwrap();
        assert!(diff.affected_files >= 1);
        assert!(
            diff.changes
                .iter()
                .any(|change| change.symbol == "added" && change.change_type == "added")
        );
        assert!(
            diff.changes
                .iter()
                .any(|change| change.symbol == "modified" && change.change_type == "modified")
        );
        assert!(
            diff.changes
                .iter()
                .any(|change| change.symbol == "deleted" && change.change_type == "deleted")
        );
    }

    #[test]
    fn diff_context_uses_merge_base_for_changed_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);

        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn base() {}\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "base"]);
        git(&repo, &["branch", "-M", "main"]);

        git(&repo, &["checkout", "-b", "feat/diff"]);
        std::fs::write(repo.join("src/feat.rs"), "pub fn feat_only() {}\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "feature change"]);

        git(&repo, &["checkout", "main"]);
        std::fs::write(repo.join("src/main_only.rs"), "pub fn main_only() {}\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "main-only change"]);

        git(&repo, &["checkout", "feat/diff"]);

        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        branch_state::upsert_branch_state(
            &conn,
            &branch_state::BranchState {
                repo: "proj".to_string(),
                r#ref: "main".to_string(),
                merge_base_commit: None,
                last_indexed_commit: "main-head".to_string(),
                overlay_dir: None,
                file_count: 2,
                symbol_count: 2,
                is_default_branch: true,
                status: "active".to_string(),
                eviction_eligible_at: None,
                created_at: "2026-02-25T00:00:00Z".to_string(),
                last_accessed_at: "2026-02-25T00:00:00Z".to_string(),
            },
        )
        .unwrap();
        branch_state::upsert_branch_state(
            &conn,
            &branch_state::BranchState {
                repo: "proj".to_string(),
                r#ref: "feat/diff".to_string(),
                merge_base_commit: None,
                last_indexed_commit: "feat-head".to_string(),
                overlay_dir: None,
                file_count: 2,
                symbol_count: 2,
                is_default_branch: false,
                status: "active".to_string(),
                eviction_eligible_at: None,
                created_at: "2026-02-25T00:00:00Z".to_string(),
                last_accessed_at: "2026-02-25T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        insert_symbol(
            &conn,
            "proj",
            "main",
            "src/main_only.rs",
            "stable-main-only",
            "sym-main-only",
            "main_only",
            "pub fn main_only() {}",
            "main only body",
        );
        insert_symbol(
            &conn,
            "proj",
            "feat/diff",
            "src/feat.rs",
            "stable-feat-only",
            "sym-feat-only",
            "feat_only",
            "pub fn feat_only() {}",
            "feat only body",
        );

        let diff = diff_context(&conn, &repo, "proj", "main", "feat/diff", None, 50).unwrap();
        assert!(
            diff.changes
                .iter()
                .any(|change| change.symbol == "feat_only" && change.change_type == "added"),
            "feature branch changes should be included"
        );
        assert!(
            !diff
                .changes
                .iter()
                .any(|change| change.symbol == "main_only" && change.change_type == "deleted"),
            "main-only changes after branch divergence must not appear in feature diff"
        );
        assert!(
            !diff
                .file_changes
                .iter()
                .any(|change| change.path == "src/main_only.rs"),
            "file list should be based on merge-base delta"
        );
    }

    #[test]
    fn diff_context_path_filter_restricts_changes() {
        let before = SymbolWithHash {
            symbol: DiffSymbolSnapshot {
                symbol_id: "a".to_string(),
                symbol_stable_id: "stable".to_string(),
                kind: "function".to_string(),
                qualified_name: "crate::a".to_string(),
                signature: Some("fn a()".to_string()),
                path: "src/lib.rs".to_string(),
                line_start: 1,
                line_end: 2,
            },
            content_hash: "1".to_string(),
            name: "a".to_string(),
        };
        let after = SymbolWithHash {
            symbol: DiffSymbolSnapshot {
                symbol_id: "a2".to_string(),
                symbol_stable_id: "stable".to_string(),
                kind: "function".to_string(),
                qualified_name: "crate::a".to_string(),
                signature: Some("fn a(v: i32)".to_string()),
                path: "src/lib.rs".to_string(),
                line_start: 1,
                line_end: 2,
            },
            content_hash: "2".to_string(),
            name: "a".to_string(),
        };
        let change = classify_symbol_change(Some(&before), Some(&after)).unwrap();
        assert_eq!(change.change_type, "modified");
        assert!(matches_path_filter(
            &DiffEntry::modified("src/lib.rs"),
            "src/"
        ));
        assert!(!matches_path_filter(
            &DiffEntry::modified("docs/readme.md"),
            "src/"
        ));
    }
}
