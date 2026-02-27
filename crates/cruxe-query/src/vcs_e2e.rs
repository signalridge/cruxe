#![cfg(unix)]

use crate::{locate, search};
use cruxe_core::constants;
use cruxe_core::time::now_iso8601;
use cruxe_core::types::{FileRecord, Project, SourceLayer};
use cruxe_core::vcs::detect_head_commit;
use cruxe_indexer::{
    languages, overlay, parser, scanner, snippet_extract, symbol_extract,
    sync_incremental::{self, BranchSyncStateUpdate, IncrementalSyncRequest},
    writer,
};
use cruxe_state::{
    db, manifest, project, schema, tantivy_index::IndexSet, tombstones as tombstone_store,
};
use cruxe_vcs::Git2VcsAdapter;
use rusqlite::Connection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn fixture_setup_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/vcs-sample/setup.sh")
}

fn setup_vcs_fixture_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempdir().expect("create tempdir");
    let script = fixture_setup_script();
    assert!(
        script.exists(),
        "missing fixture script: {}",
        script.display()
    );
    let repo_path = tmp.path().join("vcs-sample");

    let output = std::process::Command::new("bash")
        .arg(&script)
        .arg(tmp.path())
        .output()
        .expect("run fixture setup script");
    assert!(
        output.status.success(),
        "fixture setup failed:\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (tmp, repo_path)
}

fn git(repo_root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .expect("run git command");
    assert!(
        output.status.success(),
        "git {:?} failed:\nstdout:{}\nstderr:{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn insert_vcs_project(conn: &Connection, project_id: &str, repo_root: &Path) {
    let now = "2026-02-25T00:00:00Z".to_string();
    let project_row = Project {
        project_id: project_id.to_string(),
        repo_root: repo_root.to_string_lossy().to_string(),
        display_name: Some("vcs-e2e".to_string()),
        default_ref: "main".to_string(),
        vcs_mode: true,
        schema_version: constants::SCHEMA_VERSION,
        parser_version: constants::PARSER_VERSION,
        created_at: now.clone(),
        updated_at: now,
    };
    project::create_project(conn, &project_row).expect("create project row");
}

fn index_current_checkout_as_base(
    repo_root: &Path,
    data_dir: &Path,
    conn: &Connection,
    project_id: &str,
) {
    let base_index_set = IndexSet::open(data_dir).expect("open base index set");
    let files = scanner::scan_directory_filtered(
        repo_root,
        constants::MAX_FILE_SIZE,
        &["rust".to_string()],
    );
    for file in files {
        let content = std::fs::read_to_string(&file.path).expect("read fixture source file");
        let tree = parser::parse_file(&content, &file.language).expect("parse fixture source file");
        let extracted = languages::extract_symbols(&tree, &content, &file.language);
        let symbols = symbol_extract::build_symbol_records(
            &extracted,
            project_id,
            "main",
            &file.relative_path,
            None,
        );
        let snippets = snippet_extract::build_snippet_records(
            &extracted,
            project_id,
            "main",
            &file.relative_path,
            None,
        );
        let file_record = FileRecord {
            repo: project_id.to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename: file
                .path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default(),
            language: file.language.clone(),
            content_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
            size_bytes: content.len() as u64,
            updated_at: now_iso8601(),
            content_head: Some(content.lines().take(20).collect::<Vec<_>>().join("\n")),
        };
        writer::write_file_records(&base_index_set, conn, &symbols, &snippets, &file_record)
            .expect("write base file records");
    }

    let base_head = detect_head_commit(repo_root).expect("detect base head");
    sync_incremental::persist_branch_sync_state(
        conn,
        &BranchSyncStateUpdate {
            repo: project_id.to_string(),
            ref_name: "main".to_string(),
            merge_base_commit: None,
            last_indexed_commit: base_head,
            overlay_dir: None,
            file_count: manifest::file_count(conn, project_id, "main").expect("base file count")
                as i64,
            symbol_count: 0,
            is_default_branch: true,
        },
    )
    .expect("persist main branch state");
}

fn sync_branch(
    conn: &mut Connection,
    repo_root: &Path,
    data_dir: &Path,
    project_id: &str,
    ref_name: &str,
    sync_id: &str,
    last_indexed_commit: Option<&str>,
) -> sync_incremental::IncrementalSyncStats {
    let adapter = Git2VcsAdapter;
    sync_incremental::run_incremental_sync(
        &adapter,
        conn,
        IncrementalSyncRequest {
            repo_root,
            data_dir,
            project_id,
            ref_name,
            base_ref: "main",
            sync_id,
            last_indexed_commit,
            is_default_branch: false,
        },
    )
    .expect("run incremental sync")
}

fn manifest_paths(conn: &Connection, repo: &str, ref_name: &str) -> Vec<String> {
    let mut paths: Vec<String> = manifest::get_all_entries(conn, repo, ref_name)
        .expect("read manifest entries")
        .into_iter()
        .map(|entry| entry.path)
        .collect();
    paths.sort();
    paths
}

fn open_vcs_indices(data_dir: &Path, target_ref: &str) -> (IndexSet, IndexSet) {
    let base = IndexSet::open_existing(data_dir).expect("open existing base index");
    let overlay_dir = overlay::overlay_dir_for_ref(data_dir, target_ref);
    let overlay = IndexSet::open_existing_at(&overlay_dir).expect("open existing overlay index");
    (base, overlay)
}

fn tombstone_paths(conn: &Connection, project_id: &str, target_ref: &str) -> HashSet<String> {
    tombstone_store::list_paths_for_ref(conn, project_id, target_ref)
        .expect("load tombstones")
        .into_iter()
        .collect()
}

fn merged_locate(
    conn: &Connection,
    data_dir: &Path,
    project_id: &str,
    target_ref: &str,
    name: &str,
) -> Vec<locate::LocateResult> {
    let (base, overlay_index_set) = open_vcs_indices(data_dir, target_ref);
    let tombstones = tombstone_paths(conn, project_id, target_ref);
    let (results, _) = locate::locate_symbol_vcs_merged(
        locate::VcsLocateContext {
            base_index: &base.symbols,
            overlay_index: &overlay_index_set.symbols,
            tombstones: &tombstones,
            base_ref: "main",
            target_ref,
        },
        name,
        None,
        Some("rust"),
        20,
    )
    .expect("run merged locate");
    results
}

fn merged_search(
    conn: &Connection,
    data_dir: &Path,
    project_id: &str,
    target_ref: &str,
    query: &str,
) -> search::SearchResponse {
    let (base, overlay_index_set) = open_vcs_indices(data_dir, target_ref);
    let tombstones = tombstone_paths(conn, project_id, target_ref);
    search::search_code_vcs_merged(
        search::VcsSearchContext {
            base_index_set: &base,
            overlay_index_set: &overlay_index_set,
            tombstones: &tombstones,
            base_ref: "main",
            target_ref,
        },
        Some(conn),
        query,
        Some("rust"),
        20,
        false,
    )
    .expect("run merged search")
}

#[test]
fn t281_modify_signature_on_feature_branch_returns_ref_consistent_results_sc400() {
    let (tmp, repo_root) = setup_vcs_fixture_repo();
    let data_dir = tmp.path().join("data");
    let mut conn = db::open_connection(&tmp.path().join("state.db")).expect("open sqlite");
    schema::create_tables(&conn).expect("create schema");
    insert_vcs_project(&conn, "proj-vcs", &repo_root);

    git(&repo_root, &["checkout", "main"]);
    index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");

    let base_index = IndexSet::open_existing(&data_dir).expect("open base index");
    let base_results = locate::locate_symbol(
        &base_index.symbols,
        "shared",
        Some("function"),
        Some("rust"),
        Some("main"),
        10,
    )
    .expect("locate shared on main");
    let base_shared = base_results
        .iter()
        .find(|result| result.path == "src/lib.rs")
        .expect("main should include shared in src/lib.rs");
    assert!(
        !base_shared
            .signature
            .as_deref()
            .unwrap_or("")
            .contains("mode"),
        "main signature must remain pre-modification"
    );

    git(&repo_root, &["checkout", "feat/modify-sig"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/modify-sig",
        "sync-modify-sig",
        None,
    );

    let feature_results = merged_locate(&conn, &data_dir, "proj-vcs", "feat/modify-sig", "shared");
    let feature_shared = feature_results
        .iter()
        .find(|result| result.path == "src/lib.rs")
        .expect("feature ref should include shared in src/lib.rs");
    assert_eq!(feature_shared.source_layer, Some(SourceLayer::Overlay));
    assert!(
        feature_shared
            .signature
            .as_deref()
            .unwrap_or("")
            .contains("mode"),
        "feature signature should include modified parameter list"
    );
}

#[test]
fn t282_deleted_file_hidden_on_feature_and_visible_on_main_sc402() {
    let (tmp, repo_root) = setup_vcs_fixture_repo();
    let data_dir = tmp.path().join("data");
    let mut conn = db::open_connection(&tmp.path().join("state.db")).expect("open sqlite");
    schema::create_tables(&conn).expect("create schema");
    insert_vcs_project(&conn, "proj-vcs", &repo_root);

    git(&repo_root, &["checkout", "main"]);
    index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");
    let base_index = IndexSet::open_existing(&data_dir).expect("open base index");
    let main_results = locate::locate_symbol(
        &base_index.symbols,
        "keep_me",
        Some("function"),
        Some("rust"),
        Some("main"),
        10,
    )
    .expect("locate keep_me on main");
    assert!(
        main_results.iter().any(|item| item.path == "src/lib.rs"),
        "main should still surface keep_me from src/lib.rs"
    );

    git(&repo_root, &["checkout", "feat/delete-file"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/delete-file",
        "sync-delete-file",
        None,
    );

    let feature_results =
        merged_locate(&conn, &data_dir, "proj-vcs", "feat/delete-file", "keep_me");
    assert!(
        feature_results.is_empty(),
        "deleted file symbols must be suppressed for the feature ref"
    );
}

#[test]
fn t283_source_layer_is_tagged_as_base_or_overlay() {
    let (tmp, repo_root) = setup_vcs_fixture_repo();
    let data_dir = tmp.path().join("data");
    let mut conn = db::open_connection(&tmp.path().join("state.db")).expect("open sqlite");
    schema::create_tables(&conn).expect("create schema");
    insert_vcs_project(&conn, "proj-vcs", &repo_root);

    git(&repo_root, &["checkout", "main"]);
    index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");

    git(&repo_root, &["checkout", "feat/add-file"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/add-file",
        "sync-add-file",
        None,
    );

    let overlay_only = merged_search(
        &conn,
        &data_dir,
        "proj-vcs",
        "feat/add-file",
        "added_branch_file",
    );
    let overlay_result = overlay_only
        .results
        .iter()
        .find(|result| result.path == "src/add_file.rs")
        .expect("overlay query should include branch-only file");
    assert_eq!(overlay_result.source_layer, Some(SourceLayer::Overlay));

    let base_passthrough = merged_search(&conn, &data_dir, "proj-vcs", "feat/add-file", "keep_me");
    let base_result = base_passthrough
        .results
        .iter()
        .find(|result| result.path == "src/lib.rs")
        .expect("base query should include unchanged main file");
    assert_eq!(base_result.source_layer, Some(SourceLayer::Base));
}

#[test]
fn t284_overlay_dedup_wins_on_merge_key_collision() {
    let (tmp, repo_root) = setup_vcs_fixture_repo();
    let data_dir = tmp.path().join("data");
    let mut conn = db::open_connection(&tmp.path().join("state.db")).expect("open sqlite");
    schema::create_tables(&conn).expect("create schema");
    insert_vcs_project(&conn, "proj-vcs", &repo_root);

    git(&repo_root, &["checkout", "main"]);
    index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");

    git(&repo_root, &["checkout", "feat/modify-sig"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/modify-sig",
        "sync-modify-sig",
        None,
    );

    let merged = merged_locate(&conn, &data_dir, "proj-vcs", "feat/modify-sig", "shared");
    let lib_matches: Vec<_> = merged
        .iter()
        .filter(|result| result.path == "src/lib.rs")
        .collect();
    assert_eq!(
        lib_matches.len(),
        1,
        "overlay merge should deduplicate base+overlay collision to one result"
    );
    assert_eq!(lib_matches[0].source_layer, Some(SourceLayer::Overlay));
    assert!(
        lib_matches[0]
            .signature
            .as_deref()
            .unwrap_or("")
            .contains("mode"),
        "the surviving collision winner must be the overlay variant"
    );
}

#[test]
fn t290_ga_same_query_on_two_refs_is_ref_consistent_sc400() {
    let (tmp, repo_root) = setup_vcs_fixture_repo();
    let data_dir = tmp.path().join("data");
    let mut conn = db::open_connection(&tmp.path().join("state.db")).expect("open sqlite");
    schema::create_tables(&conn).expect("create schema");
    insert_vcs_project(&conn, "proj-vcs", &repo_root);

    git(&repo_root, &["checkout", "main"]);
    index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");
    let base_index = IndexSet::open_existing(&data_dir).expect("open base index");
    let main_results = locate::locate_symbol(
        &base_index.symbols,
        "shared",
        Some("function"),
        Some("rust"),
        Some("main"),
        10,
    )
    .expect("locate shared on main");
    let main_signature = main_results
        .iter()
        .find(|result| result.path == "src/lib.rs")
        .and_then(|result| result.signature.as_deref())
        .unwrap_or("")
        .to_string();

    git(&repo_root, &["checkout", "feat/modify-sig"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/modify-sig",
        "sync-ga-consistency",
        None,
    );
    let feature_results = merged_locate(&conn, &data_dir, "proj-vcs", "feat/modify-sig", "shared");
    let feature_signature = feature_results
        .iter()
        .find(|result| result.path == "src/lib.rs")
        .and_then(|result| result.signature.as_deref())
        .unwrap_or("")
        .to_string();

    assert_ne!(
        main_signature, feature_signature,
        "same symbol query on different refs must reflect ref-specific content"
    );
}

#[test]
fn t291_switching_refs_does_not_reuse_stale_overlay_sc401() {
    let (tmp, repo_root) = setup_vcs_fixture_repo();
    let data_dir = tmp.path().join("data");
    let mut conn = db::open_connection(&tmp.path().join("state.db")).expect("open sqlite");
    schema::create_tables(&conn).expect("create schema");
    insert_vcs_project(&conn, "proj-vcs", &repo_root);

    git(&repo_root, &["checkout", "main"]);
    index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");

    git(&repo_root, &["checkout", "feat/add-file"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/add-file",
        "sync-ref-a",
        None,
    );

    git(&repo_root, &["checkout", "feat/rename-file"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/rename-file",
        "sync-ref-b",
        None,
    );

    let add_ref_results = merged_search(
        &conn,
        &data_dir,
        "proj-vcs",
        "feat/add-file",
        "added_branch_file",
    );
    assert!(
        add_ref_results
            .results
            .iter()
            .any(|result| result.path == "src/add_file.rs"),
        "add-file ref should surface its overlay file"
    );
    assert!(
        add_ref_results
            .results
            .iter()
            .all(|result| result.path != "src/core.rs"),
        "add-file ref must not leak renamed-file overlay artifacts"
    );

    let rename_ref_results =
        merged_search(&conn, &data_dir, "proj-vcs", "feat/rename-file", "shared");
    assert!(
        rename_ref_results
            .results
            .iter()
            .any(|result| result.path == "src/core.rs"),
        "rename-file ref should return renamed symbol path"
    );
    assert!(
        rename_ref_results
            .results
            .iter()
            .all(|result| result.path != "src/add_file.rs"),
        "rename-file ref must not reuse add-file overlay state"
    );
}

#[test]
fn t292_ga_deleted_file_not_returned_from_base_for_feature_ref_sc402() {
    let (tmp, repo_root) = setup_vcs_fixture_repo();
    let data_dir = tmp.path().join("data");
    let mut conn = db::open_connection(&tmp.path().join("state.db")).expect("open sqlite");
    schema::create_tables(&conn).expect("create schema");
    insert_vcs_project(&conn, "proj-vcs", &repo_root);

    git(&repo_root, &["checkout", "main"]);
    index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");

    let base_index = IndexSet::open_existing(&data_dir).expect("open base index");
    let main_search = search::search_code(
        &base_index,
        Some(&conn),
        "keep_me",
        Some("main"),
        Some("rust"),
        20,
        false,
    )
    .expect("search keep_me on main");
    assert!(
        main_search
            .results
            .iter()
            .any(|result| result.path == "src/lib.rs"),
        "main must return deleted symbol before branch overlay suppression"
    );

    git(&repo_root, &["checkout", "feat/delete-file"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/delete-file",
        "sync-ga-delete",
        None,
    );
    let feature_search = merged_search(&conn, &data_dir, "proj-vcs", "feat/delete-file", "keep_me");
    assert!(
        feature_search
            .results
            .iter()
            .all(|result| result.path != "src/lib.rs"),
        "feature ref must suppress tombstoned base results"
    );
}

#[test]
fn t293_ga_rebase_after_indexing_refreshes_results_without_base_rebuild_sc403() {
    let (tmp, repo_root) = setup_vcs_fixture_repo();
    let data_dir = tmp.path().join("data");
    let mut conn = db::open_connection(&tmp.path().join("state.db")).expect("open sqlite");
    schema::create_tables(&conn).expect("create schema");
    insert_vcs_project(&conn, "proj-vcs", &repo_root);

    git(&repo_root, &["checkout", "main"]);
    index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");
    let main_manifest_before = manifest_paths(&conn, "proj-vcs", "main");

    git(&repo_root, &["checkout", "feat/rebase-target"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/rebase-target",
        "sync-rebase-1",
        None,
    );
    let old_head = git(&repo_root, &["rev-parse", "HEAD"]);

    let overlay_dir = overlay::overlay_dir_for_ref(&data_dir, "feat/rebase-target");
    std::fs::write(overlay_dir.join("stale.marker"), "stale").expect("write stale marker");

    git(&repo_root, &["rebase", "main"]);
    let second = sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/rebase-target",
        "sync-rebase-2",
        Some(&old_head),
    );
    assert!(second.rebuild_triggered);
    assert!(
        !overlay::overlay_dir_for_ref(&data_dir, "feat/rebase-target")
            .join("stale.marker")
            .exists(),
        "rebuild should replace stale overlay contents"
    );

    let refreshed = merged_search(
        &conn,
        &data_dir,
        "proj-vcs",
        "feat/rebase-target",
        "rebase_target",
    );
    assert!(
        refreshed
            .results
            .iter()
            .any(|result| result.path == "src/rebase_target.rs"),
        "rebased overlay should still return the branch-specific symbol"
    );
    assert_eq!(
        manifest_paths(&conn, "proj-vcs", "main"),
        main_manifest_before,
        "rebase recovery must not mutate the base manifest"
    );
}

#[test]
fn t295_branch_result_correctness_fixture_matrix() {
    let (tmp, repo_root) = setup_vcs_fixture_repo();
    let data_dir = tmp.path().join("data");
    let mut conn = db::open_connection(&tmp.path().join("state.db")).expect("open sqlite");
    schema::create_tables(&conn).expect("create schema");
    insert_vcs_project(&conn, "proj-vcs", &repo_root);

    git(&repo_root, &["checkout", "main"]);
    index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");

    git(&repo_root, &["checkout", "feat/add-file"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/add-file",
        "sync-matrix-add",
        None,
    );
    let add_ok = merged_search(
        &conn,
        &data_dir,
        "proj-vcs",
        "feat/add-file",
        "added_branch_file",
    )
    .results
    .iter()
    .any(|result| result.path == "src/add_file.rs");

    git(&repo_root, &["checkout", "feat/modify-sig"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/modify-sig",
        "sync-matrix-modify",
        None,
    );
    let modify_ok = merged_locate(&conn, &data_dir, "proj-vcs", "feat/modify-sig", "shared")
        .iter()
        .any(|result| {
            result.path == "src/lib.rs"
                && result.source_layer == Some(SourceLayer::Overlay)
                && result.signature.as_deref().unwrap_or("").contains("mode")
        });

    git(&repo_root, &["checkout", "feat/delete-file"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/delete-file",
        "sync-matrix-delete",
        None,
    );
    let delete_ok = merged_search(&conn, &data_dir, "proj-vcs", "feat/delete-file", "keep_me")
        .results
        .iter()
        .all(|result| result.path != "src/lib.rs");

    git(&repo_root, &["checkout", "feat/rename-file"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/rename-file",
        "sync-matrix-rename",
        None,
    );
    let rename_ok = merged_search(&conn, &data_dir, "proj-vcs", "feat/rename-file", "shared")
        .results
        .iter()
        .any(|result| result.path == "src/core.rs");

    let checks = [add_ok, modify_ok, delete_ok, rename_ok];
    assert!(
        checks.iter().all(|value| *value),
        "fixture matrix failed: add={add_ok}, modify={modify_ok}, delete={delete_ok}, rename={rename_ok}"
    );
}

#[test]
fn t296_revert_to_base_clears_stale_tombstones() {
    let (tmp, repo_root) = setup_vcs_fixture_repo();
    let data_dir = tmp.path().join("data");
    let mut conn = db::open_connection(&tmp.path().join("state.db")).expect("open sqlite");
    schema::create_tables(&conn).expect("create schema");
    insert_vcs_project(&conn, "proj-vcs", &repo_root);

    git(&repo_root, &["checkout", "main"]);
    index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");

    git(&repo_root, &["checkout", "feat/modify-sig"]);
    sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/modify-sig",
        "sync-revert-1",
        None,
    );

    let before = merged_locate(&conn, &data_dir, "proj-vcs", "feat/modify-sig", "shared");
    let before_shared = before
        .iter()
        .find(|result| result.path == "src/lib.rs")
        .expect("feature branch should return shared before revert");
    assert_eq!(before_shared.source_layer, Some(SourceLayer::Overlay));
    assert!(
        before_shared
            .signature
            .as_deref()
            .unwrap_or("")
            .contains("mode"),
        "pre-revert result should carry overlay signature"
    );

    git(
        &repo_root,
        &[
            "restore",
            "--source",
            "main",
            "--worktree",
            "--staged",
            "src/lib.rs",
        ],
    );
    git(
        &repo_root,
        &["commit", "-m", "feat/modify-sig: revert to base"],
    );
    let second_sync = sync_branch(
        &mut conn,
        &repo_root,
        &data_dir,
        "proj-vcs",
        "feat/modify-sig",
        "sync-revert-2",
        None,
    );
    assert_eq!(
        second_sync.changed_files, 0,
        "reverted branch should have no remaining merge-base diff entries"
    );

    let after = merged_locate(&conn, &data_dir, "proj-vcs", "feat/modify-sig", "shared");
    let after_shared = after
        .iter()
        .find(|result| result.path == "src/lib.rs")
        .expect("base result should be visible after reverting branch file");
    assert_eq!(after_shared.source_layer, Some(SourceLayer::Base));
    assert!(
        !after_shared
            .signature
            .as_deref()
            .unwrap_or("")
            .contains("mode"),
        "reverted branch must not keep stale overlay suppression"
    );
}
