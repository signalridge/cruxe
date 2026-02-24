use super::*;
use codecompass_core::config::Config;
use codecompass_core::types::Project;
use serde_json::json;
use std::path::Path;

/// Default prewarm status for tests (complete).
fn test_prewarm_status() -> AtomicU8 {
    AtomicU8::new(PREWARM_COMPLETE)
}

/// Default server start time for tests.
fn test_server_start() -> Instant {
    Instant::now()
}

fn make_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: method.into(),
        params,
    }
}

#[test]
fn resolve_tool_ref_falls_back_to_project_default_when_head_unavailable() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path();

    let db_path = tmp.path().join("state.db");
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    codecompass_state::schema::create_tables(&conn).unwrap();

    let project_id = "proj_test";
    let project = Project {
        project_id: project_id.to_string(),
        repo_root: workspace.to_string_lossy().to_string(),
        display_name: Some("test".to_string()),
        default_ref: "main".to_string(),
        vcs_mode: true,
        schema_version: 1,
        parser_version: 1,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    codecompass_state::project::create_project(&conn, &project).unwrap();

    // Temp dir is non-git and has no HEAD branch; should fall back to project default_ref.
    let resolved = resolve_tool_ref(None, workspace, Some(&conn), project_id);
    assert_eq!(resolved, "main");

    // Explicit argument still has top priority.
    let explicit = resolve_tool_ref(Some("feat/auth"), workspace, Some(&conn), project_id);
    assert_eq!(explicit, "feat/auth");
}

// ------------------------------------------------------------------
// T065: tools/list returns all registered tools
// ------------------------------------------------------------------

#[test]
fn t065_tools_list_returns_all_registered_tools() {
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "fake_project_id";

    let request = make_request("tools/list", json!({}));
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: None,
            schema_status: SchemaStatus::NotIndexed,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success, got error");
    let result = response.result.expect("result should be present");

    let tools = result
        .get("tools")
        .expect("result should contain 'tools'")
        .as_array()
        .expect("'tools' should be an array");

    assert_eq!(tools.len(), 10, "expected 10 tools, got {}", tools.len());

    let tool_names: Vec<&str> = tools
        .iter()
        .map(|t| t.get("name").unwrap().as_str().unwrap())
        .collect();

    let expected_names = [
        "index_repo",
        "sync_repo",
        "search_code",
        "locate_symbol",
        "get_file_outline",
        "get_symbol_hierarchy",
        "find_related_symbols",
        "get_code_context",
        "health_check",
        "index_status",
    ];
    for name in &expected_names {
        assert!(
            tool_names.contains(name),
            "missing tool: {name}; found: {tool_names:?}"
        );
    }

    for tool in tools {
        assert!(tool.get("name").is_some(), "tool missing 'name': {tool:?}");
        assert!(
            tool.get("description").is_some(),
            "tool missing 'description': {tool:?}"
        );
        assert!(
            tool.get("inputSchema").is_some(),
            "tool missing 'inputSchema': {tool:?}"
        );

        let desc = tool.get("description").unwrap().as_str().unwrap();
        assert!(!desc.is_empty(), "tool description is empty: {tool:?}");

        assert!(
            tool.get("inputSchema").unwrap().is_object(),
            "inputSchema should be an object: {tool:?}"
        );
    }
}

// ------------------------------------------------------------------
// T066: locate_symbol via JSON-RPC with an indexed fixture
// ------------------------------------------------------------------

fn build_fixture_index(tmp_dir: &std::path::Path) -> IndexSet {
    use codecompass_indexer::{
        import_extract, languages, parser, scanner, snippet_extract, symbol_extract, writer,
    };
    use codecompass_state::{db, schema, tantivy_index::IndexSet};

    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/rust-sample");
    assert!(
        fixture_dir.exists(),
        "fixture directory missing: {}",
        fixture_dir.display()
    );

    let data_dir = tmp_dir.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    let index_set = IndexSet::open(&data_dir).unwrap();

    let db_path = data_dir.join("state.db");
    let conn = db::open_connection(&db_path).unwrap();
    schema::create_tables(&conn).unwrap();

    let scanned = scanner::scan_directory(&fixture_dir, 1_048_576);
    assert!(
        !scanned.is_empty(),
        "scanner found no files in fixture directory"
    );

    let repo = "test-repo";
    let r#ref = "live";
    let mut pending_imports = Vec::new();

    for file in &scanned {
        let source = std::fs::read_to_string(&file.path).unwrap();
        let tree = match parser::parse_file(&source, &file.language) {
            Ok(t) => t,
            Err(_) => continue,
        };

        let extracted = languages::extract_symbols(&tree, &source, &file.language);
        let raw_imports =
            import_extract::extract_imports(&tree, &source, &file.language, &file.relative_path);
        let symbols = symbol_extract::build_symbol_records(
            &extracted,
            repo,
            r#ref,
            &file.relative_path,
            None,
        );
        let snippets = snippet_extract::build_snippet_records(
            &extracted,
            repo,
            r#ref,
            &file.relative_path,
            None,
        );

        let content_hash = blake3::hash(source.as_bytes()).to_hex().to_string();
        let filename = file.path.file_name().unwrap().to_string_lossy().to_string();
        let file_record = codecompass_core::types::FileRecord {
            repo: repo.to_string(),
            r#ref: r#ref.to_string(),
            commit: None,
            path: file.relative_path.clone(),
            filename,
            language: file.language.clone(),
            content_hash,
            size_bytes: source.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: source
                .lines()
                .take(10)
                .collect::<Vec<_>>()
                .join("\n")
                .into(),
        };

        writer::write_file_records(&index_set, &conn, &symbols, &snippets, &file_record).unwrap();
        pending_imports.push((file.relative_path.clone(), raw_imports));
    }

    for (path, raw_imports) in pending_imports {
        writer::replace_import_edges_for_file(&conn, repo, r#ref, &path, raw_imports).unwrap();
    }

    index_set
}

#[test]
fn t066_locate_symbol_via_jsonrpc() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test_project";

    let request = make_request(
        "tools/call",
        json!({
            "name": "locate_symbol",
            "arguments": {
                "name": "validate_token"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(
        response.error.is_none(),
        "expected success, got error: {:?}",
        response.error
    );
    let result = response.result.expect("result should be present");

    let content = result
        .get("content")
        .expect("result should have 'content'")
        .as_array()
        .expect("'content' should be an array");

    assert!(!content.is_empty(), "content array should not be empty");

    let first = &content[0];
    assert_eq!(
        first.get("type").unwrap().as_str().unwrap(),
        "text",
        "content type should be 'text'"
    );

    let text = first.get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value =
        serde_json::from_str(text).expect("text payload should be valid JSON");

    let results = payload
        .get("results")
        .expect("payload should have 'results'")
        .as_array()
        .expect("'results' should be an array");

    assert!(
        !results.is_empty(),
        "results should contain at least one match for 'validate_token'"
    );

    let vt = results
        .iter()
        .find(|r| r.get("name").unwrap().as_str().unwrap() == "validate_token")
        .expect("results should contain a 'validate_token' entry");

    assert_eq!(vt.get("kind").unwrap().as_str().unwrap(), "function");
    assert_eq!(vt.get("language").unwrap().as_str().unwrap(), "rust");
    assert!(
        vt.get("path")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("auth.rs"),
        "path should reference auth.rs"
    );
    assert!(vt.get("line_start").unwrap().as_u64().unwrap() > 0);
    assert!(vt.get("line_end").unwrap().as_u64().unwrap() > 0);
    assert!(
        !vt.get("symbol_id").unwrap().as_str().unwrap().is_empty(),
        "symbol_id should not be empty"
    );
    assert!(
        !vt.get("symbol_stable_id")
            .unwrap()
            .as_str()
            .unwrap()
            .is_empty(),
        "symbol_stable_id should not be empty"
    );

    // Verify Protocol v1 metadata
    let metadata = payload
        .get("metadata")
        .expect("payload should have 'metadata'");
    assert_eq!(
        metadata
            .get("codecompass_protocol_version")
            .unwrap()
            .as_str()
            .unwrap(),
        "1.0"
    );
    assert_eq!(
        metadata.get("ref").unwrap().as_str().unwrap(),
        "live",
        "ref should default to 'live'"
    );
}

// ------------------------------------------------------------------
// Helper: extract the results array from an MCP tool response
// ------------------------------------------------------------------

fn extract_results_from_response(response: &JsonRpcResponse) -> Vec<serde_json::Value> {
    let payload = extract_payload_from_response(response);
    payload
        .get("results")
        .expect("payload should have 'results'")
        .as_array()
        .expect("'results' should be an array")
        .clone()
}

// ------------------------------------------------------------------
// T095: locate_symbol with detail_level: "location"
// ------------------------------------------------------------------

#[test]
fn t095_locate_symbol_detail_level_location() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test_project";

    let request = make_request(
        "tools/call",
        json!({
            "name": "locate_symbol",
            "arguments": {
                "name": "validate_token",
                "detail_level": "location"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let results = extract_results_from_response(&response);
    assert!(!results.is_empty(), "should have results");

    let vt = results
        .iter()
        .find(|r| r.get("name").unwrap().as_str().unwrap() == "validate_token")
        .expect("should find validate_token");

    // Location-level fields must be present
    for field in &["path", "line_start", "line_end", "kind", "name"] {
        assert!(
            vt.get(*field).is_some(),
            "location level should include field '{}'",
            field
        );
    }

    // Identity fields should always be present
    assert!(vt.get("symbol_id").is_some(), "symbol_id should be present");
    assert!(
        vt.get("symbol_stable_id").is_some(),
        "symbol_stable_id should be present"
    );
    assert!(vt.get("score").is_some(), "score should be present");

    // Signature-only fields must NOT be present at location level
    for field in &["qualified_name", "language", "visibility"] {
        assert!(
            vt.get(*field).is_none(),
            "location level should NOT include field '{}', but it was present",
            field
        );
    }

    // Context-only fields must NOT be present
    assert!(
        vt.get("body_preview").is_none(),
        "location level should NOT include body_preview"
    );
    assert!(
        vt.get("parent").is_none(),
        "location level should NOT include parent"
    );
    assert!(
        vt.get("related_symbols").is_none(),
        "location level should NOT include related_symbols"
    );
}

// ------------------------------------------------------------------
// T096: locate_symbol with detail_level: "signature" (default)
// ------------------------------------------------------------------

#[test]
fn t096_locate_symbol_detail_level_signature_default() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test_project";

    // No explicit detail_level — defaults to "signature"
    let request = make_request(
        "tools/call",
        json!({
            "name": "locate_symbol",
            "arguments": {
                "name": "validate_token"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let results = extract_results_from_response(&response);
    assert!(!results.is_empty(), "should have results");

    let vt = results
        .iter()
        .find(|r| r.get("name").unwrap().as_str().unwrap() == "validate_token")
        .expect("should find validate_token");

    // All signature-level fields must be present
    for field in &[
        "path",
        "line_start",
        "line_end",
        "kind",
        "name",
        "qualified_name",
        "language",
    ] {
        assert!(
            vt.get(*field).is_some(),
            "signature level should include field '{}'",
            field
        );
    }

    // Identity fields should always be present
    assert!(vt.get("symbol_id").is_some(), "symbol_id should be present");
    assert!(
        vt.get("symbol_stable_id").is_some(),
        "symbol_stable_id should be present"
    );
    assert!(vt.get("score").is_some(), "score should be present");

    // Context-only fields must NOT be present at signature level
    assert!(
        vt.get("body_preview").is_none(),
        "signature level should NOT include body_preview"
    );
    assert!(
        vt.get("parent").is_none(),
        "signature level should NOT include parent"
    );
    assert!(
        vt.get("related_symbols").is_none(),
        "signature level should NOT include related_symbols"
    );
}

// ------------------------------------------------------------------
// T097: search_code with detail_level: "context"
// ------------------------------------------------------------------

#[test]
fn t097_search_code_detail_level_context() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test_project";

    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate_token",
                "detail_level": "context"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let results = extract_results_from_response(&response);
    assert!(!results.is_empty(), "should have results");

    // At context level, all standard fields pass through
    let first = &results[0];
    assert!(
        first.get("path").is_some(),
        "context level should include path"
    );
    assert!(
        first.get("line_start").is_some(),
        "context level should include line_start"
    );

    // Context level should include enrichment fields when data is available.
    // body_preview comes from snippet/content fields which are populated in search results.
    let has_body_preview = results.iter().any(|r| r.get("body_preview").is_some());
    assert!(
        has_body_preview,
        "at least one context-level result should have body_preview"
    );
}

// ------------------------------------------------------------------
// Helper: build fixture index and return both IndexSet and DB path
// ------------------------------------------------------------------

fn build_fixture_index_with_db(tmp_dir: &std::path::Path) -> (IndexSet, std::path::PathBuf) {
    let index_set = build_fixture_index(tmp_dir);
    let db_path = tmp_dir.join("data").join("state.db");
    (index_set, db_path)
}

// ------------------------------------------------------------------
// T102: get_file_outline nested tree
// ------------------------------------------------------------------

#[test]
fn t102_get_file_outline_nested_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    // types.rs has struct User + impl User with methods — good for nesting test
    let request = make_request(
        "tools/call",
        json!({
            "name": "get_file_outline",
            "arguments": {
                "path": "src/types.rs"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(
        response.error.is_none(),
        "expected success, got error: {:?}",
        response.error
    );
    let result = response.result.as_ref().expect("result should be present");
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    assert_eq!(
        payload.get("file_path").unwrap().as_str().unwrap(),
        "src/types.rs"
    );
    assert_eq!(payload.get("language").unwrap().as_str().unwrap(), "rust");

    let symbols = payload
        .get("symbols")
        .expect("should have symbols")
        .as_array()
        .expect("symbols should be an array");

    assert!(!symbols.is_empty(), "should have symbols for types.rs");

    // Verify at least one symbol has children (impl block with methods)
    let has_children = symbols.iter().any(|s| {
        let children = s.get("children").and_then(|c| c.as_array());
        children.map(|c| !c.is_empty()).unwrap_or(false)
    });
    assert!(
        has_children,
        "types.rs should have impl blocks with children (methods)"
    );

    // Verify metadata
    let metadata = payload.get("metadata").expect("should have metadata");
    assert!(metadata.get("symbol_count").unwrap().as_u64().unwrap() > 0);
}

// ------------------------------------------------------------------
// T103: get_file_outline with depth: "top"
// ------------------------------------------------------------------

#[test]
fn t103_get_file_outline_top_level_only() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "get_file_outline",
            "arguments": {
                "path": "src/types.rs",
                "depth": "top"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    let symbols = payload.get("symbols").unwrap().as_array().unwrap();

    assert!(!symbols.is_empty(), "should have top-level symbols");

    // With depth="top", no symbol should have non-empty children
    for sym in symbols {
        let children = sym.get("children").and_then(|c| c.as_array());
        let has_children = children.map(|c| !c.is_empty()).unwrap_or(false);
        assert!(
            !has_children,
            "top-level only mode should not include children, but '{}' has children",
            sym.get("name").unwrap().as_str().unwrap_or("?")
        );
    }
}

// ------------------------------------------------------------------
// T104: get_file_outline on non-existent file
// ------------------------------------------------------------------

#[test]
fn t104_get_file_outline_nonexistent_file() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "get_file_outline",
            "arguments": {
                "path": "src/nonexistent_file.rs"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "MCP tool errors are in content");
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    // Should have an error object with file_not_found code
    let error = payload.get("error").expect("should have error object");
    assert_eq!(
        error.get("code").unwrap().as_str().unwrap(),
        "file_not_found"
    );
}

#[test]
fn t104_get_file_outline_existing_file_without_symbols_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    codecompass_state::manifest::upsert_manifest(
        &conn,
        &codecompass_state::manifest::ManifestEntry {
            repo: "test-repo".to_string(),
            r#ref: "live".to_string(),
            path: "docs/README.md".to_string(),
            content_hash: blake3::hash(b"hello").to_hex().to_string(),
            size_bytes: 5,
            mtime_ns: None,
            language: Some("markdown".to_string()),
            indexed_at: "2026-01-01T00:00:00Z".to_string(),
        },
    )
    .unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "get_file_outline",
            "arguments": {
                "path": "docs/README.md",
                "language": "markdown"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    assert!(
        payload.get("error").is_none(),
        "existing file without symbols should not return file_not_found"
    );
    assert_eq!(
        payload.get("file_path").unwrap().as_str().unwrap(),
        "docs/README.md"
    );
    let symbols = payload.get("symbols").unwrap().as_array().unwrap();
    assert!(symbols.is_empty(), "symbols should be empty");
}

#[test]
fn t165_get_symbol_hierarchy_ancestors_via_jsonrpc() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "get_symbol_hierarchy",
            "arguments": {
                "symbol_name": "authenticate",
                "path": "src/handler.rs",
                "direction": "ancestors"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let payload = extract_payload_from_response(&response);
    assert_eq!(
        payload.get("direction").unwrap().as_str().unwrap(),
        "ancestors"
    );

    let hierarchy = payload.get("hierarchy").unwrap().as_array().unwrap();
    assert!(!hierarchy.is_empty(), "hierarchy should not be empty");
    assert_eq!(
        hierarchy[0].get("name").unwrap().as_str().unwrap(),
        "authenticate"
    );
    let names = hierarchy
        .iter()
        .filter_map(|item| item.get("name").and_then(|v| v.as_str()))
        .collect::<Vec<_>>();
    assert!(
        names.contains(&"AuthHandler"),
        "ancestor chain should include AuthHandler; got {:?}",
        names
    );

    let metadata = payload.get("metadata").unwrap();
    assert_eq!(
        metadata
            .get("codecompass_protocol_version")
            .unwrap()
            .as_str()
            .unwrap(),
        "1.0"
    );
}

#[test]
fn t166_get_symbol_hierarchy_descendants_via_jsonrpc() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "get_symbol_hierarchy",
            "arguments": {
                "symbol_name": "AuthHandler",
                "path": "src/handler.rs",
                "direction": "descendants"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let payload = extract_payload_from_response(&response);
    assert_eq!(
        payload.get("direction").unwrap().as_str().unwrap(),
        "descendants"
    );
    let hierarchy = payload.get("hierarchy").unwrap().as_array().unwrap();
    assert_eq!(
        hierarchy.len(),
        1,
        "descendants should return a single root"
    );
    let children = hierarchy[0]
        .get("children")
        .unwrap()
        .as_array()
        .expect("children should be an array");
    let child_names = children
        .iter()
        .filter_map(|item| item.get("name").and_then(|v| v.as_str()))
        .collect::<Vec<_>>();
    assert!(
        child_names.contains(&"handle_request"),
        "descendants should include handle_request"
    );
    assert!(
        child_names.contains(&"authenticate"),
        "descendants should include authenticate"
    );
}

#[test]
fn t175_find_related_symbols_scope_file_via_jsonrpc() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "find_related_symbols",
            "arguments": {
                "symbol_name": "authenticate",
                "path": "src/handler.rs",
                "scope": "file",
                "limit": 20
            }
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let payload = extract_payload_from_response(&response);
    let related = payload.get("related").unwrap().as_array().unwrap();
    assert!(
        !related.is_empty(),
        "file scope should return sibling symbols"
    );
    assert!(
        related
            .iter()
            .all(|item| item.get("relation").unwrap().as_str().unwrap() == "same_file"),
        "file scope should only return same_file relation"
    );
}

#[test]
fn t176_find_related_symbols_scope_module_includes_imported() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    // Add one synthetic symbol outside src/ so module-scope lookup cannot
    // discover it via path prefix, only via import edges.
    let synthetic_symbol = codecompass_core::types::SymbolRecord {
        repo: project_id.to_string(),
        r#ref: "live".to_string(),
        commit: None,
        path: "vendor/external.rs".to_string(),
        language: "rust".to_string(),
        symbol_id: codecompass_core::types::compute_symbol_id(
            project_id,
            "live",
            "vendor/external.rs",
            &codecompass_core::types::SymbolKind::Function,
            1,
            "external_helper",
        ),
        symbol_stable_id: codecompass_core::types::compute_symbol_stable_id(
            "rust",
            &codecompass_core::types::SymbolKind::Function,
            "vendor::external_helper",
            None,
        ),
        name: "external_helper".to_string(),
        qualified_name: "vendor::external_helper".to_string(),
        kind: codecompass_core::types::SymbolKind::Function,
        signature: Some("fn external_helper()".to_string()),
        line_start: 1,
        line_end: 3,
        parent_symbol_id: None,
        visibility: Some("pub".to_string()),
        content: Some("fn external_helper() {}".to_string()),
    };
    codecompass_state::symbols::insert_symbol(&conn, &synthetic_symbol).unwrap();

    let edge = codecompass_core::types::SymbolEdge {
        repo: project_id.to_string(),
        ref_name: "live".to_string(),
        from_symbol_id: codecompass_indexer::import_extract::source_symbol_id_for_path(
            "src/handler.rs",
        ),
        to_symbol_id: synthetic_symbol.symbol_stable_id.clone(),
        edge_type: "imports".to_string(),
        confidence: "static".to_string(),
    };
    codecompass_state::edges::insert_edges(&conn, project_id, "live", vec![edge]).unwrap();

    let request = make_request(
        "tools/call",
        json!({
            "name": "find_related_symbols",
            "arguments": {
                "symbol_name": "handle_request",
                "path": "src/handler.rs",
                "scope": "module",
                "limit": 200
            }
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let payload = extract_payload_from_response(&response);
    let related = payload.get("related").unwrap().as_array().unwrap();
    let relations = related
        .iter()
        .filter_map(|item| item.get("relation").and_then(|v| v.as_str()))
        .collect::<Vec<_>>();
    assert!(
        relations.contains(&"same_module"),
        "module scope should include same_module symbols"
    );
    assert!(
        relations.contains(&"imported"),
        "module scope should include imported symbols when edges exist; got {:?}",
        relations
    );
    assert!(
        related.iter().any(|item| {
            item.get("name").and_then(|v| v.as_str()) == Some("external_helper")
                && item.get("relation").and_then(|v| v.as_str()) == Some("imported")
        }),
        "expected external_helper to appear as imported symbol"
    );
}

#[test]
fn t185_get_code_context_breadth_respects_budget() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "get_code_context",
            "arguments": {
                "query": "validate_token",
                "max_tokens": 500,
                "strategy": "breadth",
                "language": "rust"
            }
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let payload = extract_payload_from_response(&response);
    assert!(
        payload.get("estimated_tokens").unwrap().as_u64().unwrap() <= 500,
        "estimated tokens should respect max_tokens budget"
    );
    let items = payload.get("context_items").unwrap().as_array().unwrap();
    assert!(
        !items.is_empty(),
        "breadth strategy should return context items"
    );
    assert!(
        items[0].get("body").is_none(),
        "breadth strategy should not include full body"
    );
}

#[test]
fn t186_and_t187_get_code_context_depth_and_truncation() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let depth_request = make_request(
        "tools/call",
        json!({
            "name": "get_code_context",
            "arguments": {
                "query": "validate_token",
                "max_tokens": 1200,
                "strategy": "depth",
                "language": "rust"
            }
        }),
    );
    let depth_response = handle_request_with_ctx(
        &depth_request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(depth_response.error.is_none(), "expected success");
    let depth_payload = extract_payload_from_response(&depth_response);
    assert!(
        depth_payload
            .get("estimated_tokens")
            .unwrap()
            .as_u64()
            .unwrap()
            <= 1200,
        "depth response should stay under max_tokens"
    );
    let depth_items = depth_payload
        .get("context_items")
        .unwrap()
        .as_array()
        .unwrap();
    assert!(
        !depth_items.is_empty(),
        "depth strategy should return items"
    );
    assert!(
        depth_items[0]
            .get("body")
            .and_then(|v| v.as_str())
            .is_some_and(|body| !body.is_empty()),
        "depth strategy should include body text"
    );

    let tiny_budget_request = make_request(
        "tools/call",
        json!({
            "name": "get_code_context",
            "arguments": {
                "query": "validate_token",
                "max_tokens": 50,
                "strategy": "depth",
                "language": "rust"
            }
        }),
    );
    let tiny_budget_response = handle_request_with_ctx(
        &tiny_budget_request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(tiny_budget_response.error.is_none(), "expected success");
    let tiny_payload = extract_payload_from_response(&tiny_budget_response);
    assert!(
        tiny_payload.get("truncated").unwrap().as_bool().unwrap(),
        "very small max_tokens should trigger truncation"
    );
    assert!(
        tiny_payload
            .get("estimated_tokens")
            .unwrap()
            .as_u64()
            .unwrap()
            <= 50,
        "estimated tokens should never exceed tiny budget"
    );
}

#[test]
fn t191_new_tools_error_codes_follow_protocol_registry() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let missing_symbol_request = make_request(
        "tools/call",
        json!({
            "name": "get_symbol_hierarchy",
            "arguments": {
                "symbol_name": "this_symbol_should_not_exist",
                "path": "src/handler.rs"
            }
        }),
    );
    let missing_symbol_response = handle_request_with_ctx(
        &missing_symbol_request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(
        missing_symbol_response.error.is_none(),
        "MCP tool errors are in content"
    );
    let missing_symbol_payload = extract_payload_from_response(&missing_symbol_response);
    let missing_symbol_error = missing_symbol_payload
        .get("error")
        .expect("missing symbol response should contain error");
    assert_eq!(
        missing_symbol_error
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap(),
        "symbol_not_found"
    );

    let invalid_strategy_request = make_request(
        "tools/call",
        json!({
            "name": "get_code_context",
            "arguments": {
                "query": "validate_token",
                "strategy": "wide"
            }
        }),
    );
    let invalid_strategy_response = handle_request_with_ctx(
        &invalid_strategy_request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(
        invalid_strategy_response.error.is_none(),
        "MCP tool errors are in content"
    );
    let invalid_strategy_payload = extract_payload_from_response(&invalid_strategy_response);
    let invalid_strategy_error = invalid_strategy_payload
        .get("error")
        .expect("invalid strategy response should contain error");
    assert_eq!(
        invalid_strategy_error
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap(),
        "invalid_strategy"
    );

    for payload in [&missing_symbol_payload, &invalid_strategy_payload] {
        let metadata = payload
            .get("metadata")
            .expect("error responses should include protocol metadata");
        assert!(metadata.get("codecompass_protocol_version").is_some());
        assert!(metadata.get("freshness_status").is_some());
        assert!(metadata.get("indexing_status").is_some());
        assert!(metadata.get("result_completeness").is_some());
        assert!(metadata.get("ref").is_some());
    }
}

#[test]
fn t191_get_code_context_negative_max_tokens_returns_invalid_max_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "get_code_context",
            "arguments": {
                "query": "validate_token",
                "max_tokens": -1
            }
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "MCP tool errors are in content");
    let payload = extract_payload_from_response(&response);
    let error = payload
        .get("error")
        .expect("invalid max_tokens response should contain error");
    assert_eq!(
        error.get("code").and_then(|v| v.as_str()).unwrap(),
        "invalid_max_tokens"
    );
}

#[test]
fn t194_symbol_hierarchy_validation_precision() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let cases = vec![
        ("validate_token", "src/auth.rs"),
        ("require_role", "src/auth.rs"),
        ("is_expired", "src/auth.rs"),
        ("issuer", "src/auth.rs"),
        ("handle_request", "src/handler.rs"),
        ("authenticate", "src/handler.rs"),
        ("get_user", "src/handler.rs"),
        ("create_user", "src/handler.rs"),
        ("new", "src/lib.rs"),
        ("health_check", "src/lib.rs"),
    ];

    let mut correct = 0usize;
    for (symbol_name, path) in &cases {
        let request = make_request(
            "tools/call",
            json!({
                "name": "get_symbol_hierarchy",
                "arguments": {
                    "symbol_name": symbol_name,
                    "path": path,
                    "direction": "ancestors"
                }
            }),
        );
        let response = handle_request_with_ctx(
            &request,
            &RequestContext {
                config: &config,
                index_set: Some(&index_set),
                schema_status: SchemaStatus::Compatible,
                compatibility_reason: None,
                conn: Some(&conn),
                workspace,
                project_id,
                prewarm_status: &test_prewarm_status(),
                server_start: &test_server_start(),
            },
        );
        if response.error.is_some() {
            continue;
        }
        let payload = extract_payload_from_response(&response);
        let first_name = payload
            .get("hierarchy")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str());
        if first_name == Some(*symbol_name) {
            correct += 1;
        }
    }

    let precision = correct as f64 / cases.len() as f64;
    assert!(
        precision >= 0.95,
        "hierarchy precision should be >= 95%, got {:.1}% ({}/{})",
        precision * 100.0,
        correct,
        cases.len()
    );
}

#[test]
fn t195_code_context_budget_validation() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let budgets = [50_u64, 75, 100, 150, 200, 300, 500, 800, 1200, 2000];
    for budget in budgets {
        let request = make_request(
            "tools/call",
            json!({
                "name": "get_code_context",
                "arguments": {
                    "query": "validate_token",
                    "max_tokens": budget,
                    "strategy": "breadth",
                    "language": "rust"
                }
            }),
        );
        let response = handle_request_with_ctx(
            &request,
            &RequestContext {
                config: &config,
                index_set: Some(&index_set),
                schema_status: SchemaStatus::Compatible,
                compatibility_reason: None,
                conn: Some(&conn),
                workspace,
                project_id,
                prewarm_status: &test_prewarm_status(),
                server_start: &test_server_start(),
            },
        );
        assert!(response.error.is_none(), "context request should succeed");
        let payload = extract_payload_from_response(&response);
        let estimated = payload
            .get("estimated_tokens")
            .and_then(|v| v.as_u64())
            .expect("estimated_tokens should be present");
        assert!(
            estimated <= budget,
            "estimated_tokens={} should not exceed budget={}",
            estimated,
            budget
        );
    }
}

/// T111: health_check on a healthy system returns ready status
#[test]
fn t111_health_check_on_healthy_system() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "health_check",
            "arguments": {}
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success, got error");
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    // Status should be "ready"
    assert_eq!(
        payload.get("status").unwrap().as_str().unwrap(),
        "ready",
        "expected status 'ready'"
    );

    // Tantivy should be ok
    assert!(
        payload.get("tantivy_ok").unwrap().as_bool().unwrap(),
        "expected tantivy_ok: true"
    );

    // SQLite should be ok
    assert!(
        payload.get("sqlite_ok").unwrap().as_bool().unwrap(),
        "expected sqlite_ok: true"
    );

    // Grammars: all 4 should be available
    let grammars = payload.get("grammars").unwrap();
    let available = grammars.get("available").unwrap().as_array().unwrap();
    assert!(
        available.len() >= 4,
        "expected at least 4 grammars available, got {}",
        available.len()
    );
    let missing = grammars.get("missing").unwrap().as_array().unwrap();
    assert!(
        missing.is_empty(),
        "expected no missing grammars, got {:?}",
        missing
    );

    // Startup checks
    let startup = payload.get("startup_checks").unwrap();
    let index_check = startup.get("index").unwrap();
    assert_eq!(
        index_check.get("status").unwrap().as_str().unwrap(),
        "compatible"
    );

    // Protocol version in metadata
    let meta = payload.get("metadata").unwrap();
    assert!(meta.get("codecompass_protocol_version").is_some());

    // Prewarm status should be present
    assert!(payload.get("prewarm_status").is_some());
}

#[test]
fn t111_health_check_active_job_sets_indexing_status() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let project = Project {
        project_id: project_id.to_string(),
        repo_root: workspace.to_string_lossy().to_string(),
        display_name: Some("test".to_string()),
        default_ref: "live".to_string(),
        vcs_mode: false,
        schema_version: codecompass_core::constants::SCHEMA_VERSION,
        parser_version: codecompass_core::constants::PARSER_VERSION,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    codecompass_state::project::create_project(&conn, &project).unwrap();

    codecompass_state::jobs::create_job(
        &conn,
        &codecompass_state::jobs::IndexJob {
            job_id: "job_active".to_string(),
            project_id: project_id.to_string(),
            r#ref: "live".to_string(),
            mode: "incremental".to_string(),
            head_commit: Some("abc123".to_string()),
            sync_id: None,
            status: "running".to_string(),
            changed_files: 1,
            duration_ms: None,
            error_message: None,
            retry_count: 0,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        },
    )
    .unwrap();

    let request = make_request(
        "tools/call",
        json!({
            "name": "health_check",
            "arguments": {}
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &Config::default(),
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    assert_eq!(payload.get("status").unwrap().as_str().unwrap(), "indexing");
    assert!(
        payload.get("active_job").is_some(),
        "active_job should be present when an indexing job is running"
    );
}

/// T116: health_check returns "warming" when prewarm is in progress
#[test]
fn t116_health_check_warming_status_during_prewarm() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    // Simulate prewarm in progress
    let pw = AtomicU8::new(PREWARM_IN_PROGRESS);

    let request = make_request(
        "tools/call",
        json!({
            "name": "health_check",
            "arguments": {}
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &pw,
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none());
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    // Status should be "warming" while prewarm is in progress
    assert_eq!(
        payload.get("status").unwrap().as_str().unwrap(),
        "warming",
        "expected status 'warming' during prewarm"
    );
    assert_eq!(
        payload.get("prewarm_status").unwrap().as_str().unwrap(),
        "warming"
    );

    // Now simulate prewarm complete
    pw.store(PREWARM_COMPLETE, Ordering::Release);

    let response2 = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &pw,
            server_start: &test_server_start(),
        },
    );

    let result2 = response2.result.as_ref().unwrap();
    let content2 = result2.get("content").unwrap().as_array().unwrap();
    let text2 = content2[0].get("text").unwrap().as_str().unwrap();
    let payload2: serde_json::Value = serde_json::from_str(text2).unwrap();

    // Status should now be "ready"
    assert_eq!(
        payload2.get("status").unwrap().as_str().unwrap(),
        "ready",
        "expected status 'ready' after prewarm completes"
    );
    assert_eq!(
        payload2.get("prewarm_status").unwrap().as_str().unwrap(),
        "complete"
    );
}

/// T117: health_check returns "ready" immediately with --no-prewarm (skipped)
#[test]
fn t117_health_check_no_prewarm_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    // Simulate --no-prewarm: status is SKIPPED
    let pw = AtomicU8::new(PREWARM_SKIPPED);

    let request = make_request(
        "tools/call",
        json!({
            "name": "health_check",
            "arguments": {}
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &pw,
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none());
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    // Status should be "ready" immediately (not "warming")
    assert_eq!(
        payload.get("status").unwrap().as_str().unwrap(),
        "ready",
        "expected status 'ready' with --no-prewarm"
    );
    assert_eq!(
        payload.get("prewarm_status").unwrap().as_str().unwrap(),
        "skipped"
    );
}

#[test]
fn t118_health_check_prewarm_failed_reports_error() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";
    let pw = AtomicU8::new(PREWARM_FAILED);

    let request = make_request(
        "tools/call",
        json!({
            "name": "health_check",
            "arguments": {}
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &pw,
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none());
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    assert_eq!(
        payload.get("status").unwrap().as_str().unwrap(),
        "error",
        "expected status 'error' when prewarm fails"
    );
    assert_eq!(
        payload.get("prewarm_status").unwrap().as_str().unwrap(),
        "failed"
    );
}

#[test]
fn t119_health_check_not_indexed_registered_project_sets_error() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let workspace = Path::new("/tmp/fake-workspace");
    let current_project_id = "test-repo";
    let missing_project_id = "missing-proj-never-indexed";

    let current = Project {
        project_id: current_project_id.to_string(),
        repo_root: workspace.to_string_lossy().to_string(),
        display_name: Some("current".to_string()),
        default_ref: "live".to_string(),
        vcs_mode: false,
        schema_version: codecompass_core::constants::SCHEMA_VERSION,
        parser_version: codecompass_core::constants::PARSER_VERSION,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    codecompass_state::project::create_project(&conn, &current).unwrap();

    let missing = Project {
        project_id: missing_project_id.to_string(),
        repo_root: "/tmp/missing-workspace".to_string(),
        display_name: Some("missing".to_string()),
        default_ref: "live".to_string(),
        vcs_mode: false,
        schema_version: codecompass_core::constants::SCHEMA_VERSION,
        parser_version: codecompass_core::constants::PARSER_VERSION,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    codecompass_state::project::create_project(&conn, &missing).unwrap();

    let mut config = Config::default();
    config.storage.data_dir = tmp.path().join("health-data").to_string_lossy().to_string();

    let request = make_request(
        "tools/call",
        json!({
            "name": "health_check",
            "arguments": {}
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id: current_project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    assert_eq!(
        payload.get("status").unwrap().as_str().unwrap(),
        "error",
        "overall status should be error when any registered project is not indexed"
    );
    let projects = payload.get("projects").unwrap().as_array().unwrap();
    let missing_status = projects
        .iter()
        .find(|p| p.get("project_id").and_then(|v| v.as_str()) == Some(missing_project_id))
        .and_then(|p| p.get("index_status"))
        .and_then(|v| v.as_str());
    assert_eq!(missing_status, Some("error"));
}

/// T122: search_code with debug ranking_reasons enabled returns per-result explanations
#[test]
fn t122_search_code_ranking_reasons_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let mut config = Config::default();
    config.debug.ranking_reasons = true;
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    // ranking_reasons should be present in metadata
    let meta = payload.get("metadata").unwrap();
    let reasons = meta
        .get("ranking_reasons")
        .expect("ranking_reasons should be present in metadata when debug is enabled");
    let reasons_array = reasons.as_array().unwrap();

    // Should have one entry per result
    let results = payload.get("results").unwrap().as_array().unwrap();
    assert_eq!(
        reasons_array.len(),
        results.len(),
        "ranking_reasons should have one entry per result"
    );

    // Each reason should have all 7 fields
    if let Some(first) = reasons_array.first() {
        assert!(first.get("result_index").is_some(), "missing result_index");
        assert!(
            first.get("exact_match_boost").is_some(),
            "missing exact_match_boost"
        );
        assert!(
            first.get("qualified_name_boost").is_some(),
            "missing qualified_name_boost"
        );
        assert!(
            first.get("path_affinity").is_some(),
            "missing path_affinity"
        );
        assert!(
            first.get("definition_boost").is_some(),
            "missing definition_boost"
        );
        assert!(first.get("kind_match").is_some(), "missing kind_match");
        assert!(first.get("bm25_score").is_some(), "missing bm25_score");
        assert!(first.get("final_score").is_some(), "missing final_score");
    }
}

/// T123: search_code with debug ranking_reasons disabled (default) omits ranking_reasons
#[test]
fn t123_search_code_ranking_reasons_disabled() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let config = Config::default(); // ranking_reasons defaults to false
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let result = response.result.as_ref().unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();

    // ranking_reasons should be absent from metadata
    let meta = payload.get("metadata").unwrap();
    assert!(
        meta.get("ranking_reasons").is_none(),
        "ranking_reasons should be absent when debug is disabled"
    );
}

/// T124: search_code with ranking_explain_level=basic returns compact factors
#[test]
fn t124_search_code_ranking_reasons_basic_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate",
                "ranking_explain_level": "basic"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let payload = extract_payload_from_response(&response);
    let meta = payload.get("metadata").unwrap();
    let reasons = meta
        .get("ranking_reasons")
        .expect("ranking_reasons should be present for basic mode")
        .as_array()
        .unwrap();
    assert!(!reasons.is_empty(), "expected non-empty ranking_reasons");
    let first = reasons.first().unwrap();
    assert!(first.get("exact_match").is_some());
    assert!(first.get("path_boost").is_some());
    assert!(first.get("definition_boost").is_some());
    assert!(first.get("semantic_similarity").is_some());
    assert!(first.get("final_score").is_some());
    assert!(
        first.get("exact_match_boost").is_none(),
        "basic mode must omit full explainability fields"
    );
}

/// T125: compact=true keeps identity fields and omits heavy context fields
#[test]
fn t125_search_code_compact_context_omits_heavy_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let base_request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate",
                "detail_level": "context"
            }
        }),
    );
    let base_response = handle_request_with_ctx(
        &base_request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(base_response.error.is_none(), "expected baseline success");
    let base_payload = extract_payload_from_response(&base_response);
    let base_results = base_payload.get("results").unwrap().as_array().unwrap();
    assert!(!base_results.is_empty(), "expected baseline results");

    let has_heavy_field = base_results.iter().any(|item| {
        item.get("snippet").is_some()
            || item.get("body_preview").is_some()
            || item.get("parent").is_some()
            || item.get("related_symbols").is_some()
    });
    assert!(
        has_heavy_field,
        "baseline context response should contain at least one heavy field"
    );

    let compact_request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate",
                "detail_level": "context",
                "compact": true
            }
        }),
    );
    let compact_response = handle_request_with_ctx(
        &compact_request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(compact_response.error.is_none(), "expected compact success");
    let compact_payload = extract_payload_from_response(&compact_response);
    let compact_results = compact_payload.get("results").unwrap().as_array().unwrap();
    assert!(!compact_results.is_empty(), "expected compact results");

    for item in compact_results {
        assert!(
            item.get("result_id").is_some(),
            "compact mode must preserve stable handles"
        );
        assert!(
            item.get("path").is_some(),
            "compact mode must preserve location"
        );
        assert!(
            item.get("score").is_some(),
            "compact mode must preserve score"
        );
        assert!(
            item.get("snippet").is_none()
                && item.get("body_preview").is_none()
                && item.get("parent").is_none()
                && item.get("related_symbols").is_none(),
            "compact mode must omit heavy optional fields"
        );
    }
}

/// T126: payload safety limit truncates gracefully with metadata marker
#[test]
fn t126_search_code_payload_safety_limit_truncates() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let mut config = Config::default();
    config.search.max_response_bytes = 64;
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate",
                "detail_level": "context"
            }
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let payload = extract_payload_from_response(&response);
    let metadata = payload.get("metadata").unwrap();
    assert_eq!(
        metadata
            .get("result_completeness")
            .unwrap()
            .as_str()
            .unwrap(),
        "truncated"
    );
    assert_eq!(metadata.get("safety_limit_applied"), Some(&json!(true)));

    let actions = payload
        .get("suggested_next_actions")
        .and_then(|v| v.as_array())
        .expect("suggested_next_actions should exist");
    assert!(
        !actions.is_empty(),
        "truncation should provide follow-up suggestions"
    );
}

/// T127: search_code reports dedup metadata and preserves pre-dedup candidate count
#[test]
fn t127_search_code_reports_suppressed_duplicate_count() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());
    let db_path = tmp.path().join("data/state.db");
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate_token",
                "limit": 20
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let payload = extract_payload_from_response(&response);

    let results_len = payload
        .get("results")
        .and_then(|v| v.as_array())
        .map(std::vec::Vec::len)
        .unwrap_or(0);
    let total_candidates = payload
        .get("total_candidates")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    assert!(
        total_candidates >= results_len,
        "total_candidates should be pre-dedup/pre-truncation count"
    );

    let metadata = payload.get("metadata").unwrap();
    let suppressed = metadata
        .get("suppressed_duplicate_count")
        .and_then(|v| v.as_u64())
        .expect("expected dedup metadata for duplicate-heavy query");
    assert!(suppressed > 0, "suppressed_duplicate_count should be > 0");
}

/// T128: locate_symbol safety truncation returns deterministic follow-up actions
#[test]
fn t128_locate_symbol_payload_safety_limit_has_followups() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let mut config = Config::default();
    config.search.max_response_bytes = 64;
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "locate_symbol",
            "arguments": {
                "name": "validate_token",
                "detail_level": "context",
                "limit": 10
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let payload = extract_payload_from_response(&response);
    let metadata = payload.get("metadata").unwrap();
    assert_eq!(
        metadata
            .get("result_completeness")
            .unwrap()
            .as_str()
            .unwrap(),
        "truncated"
    );
    assert_eq!(metadata.get("safety_limit_applied"), Some(&json!(true)));

    let actions = payload
        .get("suggested_next_actions")
        .and_then(|v| v.as_array())
        .expect("locate_symbol truncation should provide suggested_next_actions");
    assert!(
        actions.len() >= 2,
        "locate_symbol truncation should provide deterministic follow-ups"
    );
    assert_eq!(
        actions[0].get("tool").and_then(|v| v.as_str()),
        Some("locate_symbol")
    );
}

// ------------------------------------------------------------------
// T134: tools/list schema verification for get_file_outline + health_check
// ------------------------------------------------------------------

#[test]
fn t134_tools_list_schema_verification() {
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "fake_project_id";

    let request = make_request("tools/list", json!({}));
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: None,
            schema_status: SchemaStatus::NotIndexed,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    let result = response.result.unwrap();
    let tools = result.get("tools").unwrap().as_array().unwrap();

    // Verify get_file_outline schema
    let outline = tools
        .iter()
        .find(|t| t.get("name").unwrap().as_str().unwrap() == "get_file_outline")
        .expect("get_file_outline should be listed");
    let outline_schema = outline.get("inputSchema").unwrap();
    let outline_props = outline_schema.get("properties").unwrap();
    assert!(
        outline_props.get("path").is_some(),
        "get_file_outline should have 'path' property"
    );
    let outline_required = outline_schema.get("required").unwrap().as_array().unwrap();
    assert!(
        outline_required.contains(&json!("path")),
        "get_file_outline should require 'path'"
    );

    // Verify health_check schema
    let health = tools
        .iter()
        .find(|t| t.get("name").unwrap().as_str().unwrap() == "health_check")
        .expect("health_check should be listed");
    let health_schema = health.get("inputSchema").unwrap();
    let health_props = health_schema.get("properties").unwrap();
    assert!(
        health_props.get("workspace").is_some(),
        "health_check should have 'workspace' property"
    );

    let hierarchy = tools
        .iter()
        .find(|t| t.get("name").unwrap().as_str().unwrap() == "get_symbol_hierarchy")
        .expect("get_symbol_hierarchy should be listed");
    let hierarchy_schema = hierarchy.get("inputSchema").unwrap();
    let hierarchy_props = hierarchy_schema.get("properties").unwrap();
    assert!(hierarchy_props.get("symbol_name").is_some());
    assert!(
        hierarchy_props
            .get("direction")
            .and_then(|v| v.get("enum"))
            .is_some(),
        "get_symbol_hierarchy direction should define enum values"
    );
    assert!(
        hierarchy_props.get("compact").is_none(),
        "003 tools must not expose compact parameter in this phase"
    );
    let hierarchy_required = hierarchy_schema
        .get("required")
        .unwrap()
        .as_array()
        .unwrap();
    assert!(hierarchy_required.contains(&json!("symbol_name")));

    let related = tools
        .iter()
        .find(|t| t.get("name").unwrap().as_str().unwrap() == "find_related_symbols")
        .expect("find_related_symbols should be listed");
    let related_schema = related.get("inputSchema").unwrap();
    let related_props = related_schema.get("properties").unwrap();
    assert!(related_props.get("scope").is_some());
    assert!(related_props.get("limit").is_some());
    assert!(
        related_props.get("compact").is_none(),
        "003 tools must not expose compact parameter in this phase"
    );
    let related_required = related_schema.get("required").unwrap().as_array().unwrap();
    assert!(related_required.contains(&json!("symbol_name")));

    let context = tools
        .iter()
        .find(|t| t.get("name").unwrap().as_str().unwrap() == "get_code_context")
        .expect("get_code_context should be listed");
    let context_schema = context.get("inputSchema").unwrap();
    let context_props = context_schema.get("properties").unwrap();
    assert!(context_props.get("query").is_some());
    assert!(context_props.get("max_tokens").is_some());
    assert!(context_props.get("strategy").is_some());
    assert!(
        context_props.get("compact").is_none(),
        "003 tools must not expose compact parameter in this phase"
    );
    let context_required = context_schema.get("required").unwrap().as_array().unwrap();
    assert!(context_required.contains(&json!("query")));
}

// ------------------------------------------------------------------
// T135: Full E2E workflow test
// ------------------------------------------------------------------

#[test]
fn t135_full_e2e_workflow() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    // Open a DB connection for outline queries
    let db_path = tmp.path().join("data/state.db");
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    // Must match the repo name used in build_fixture_index
    let project_id = "test-repo";

    // Step 1: health_check
    let request = make_request(
        "tools/call",
        json!({
            "name": "health_check",
            "arguments": {}
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(response.error.is_none(), "health_check should succeed");
    let payload = extract_payload_from_response(&response);
    assert_eq!(
        payload.get("status").unwrap().as_str().unwrap(),
        "ready",
        "health_check should report 'ready'"
    );

    // Step 2: locate_symbol with detail_level: "location"
    let request = make_request(
        "tools/call",
        json!({
            "name": "locate_symbol",
            "arguments": {
                "name": "validate_token",
                "detail_level": "location"
            }
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(
        response.error.is_none(),
        "locate_symbol(location) should succeed"
    );
    let results = extract_results_from_response(&response);
    assert!(!results.is_empty(), "locate should find validate_token");
    let vt = &results[0];
    assert!(
        vt.get("path").is_some(),
        "location level should include path"
    );
    assert!(
        vt.get("qualified_name").is_none(),
        "location level should NOT include qualified_name"
    );

    // Step 3: get_file_outline for the found file
    let found_path = vt.get("path").unwrap().as_str().unwrap();
    let request = make_request(
        "tools/call",
        json!({
            "name": "get_file_outline",
            "arguments": {
                "path": found_path
            }
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(response.error.is_none(), "get_file_outline should succeed");
    let payload = extract_payload_from_response(&response);
    assert!(
        payload.get("symbols").is_some(),
        "get_file_outline should return symbols"
    );
    let symbols = payload.get("symbols").unwrap().as_array().unwrap();
    assert!(
        !symbols.is_empty(),
        "get_file_outline should return at least one symbol"
    );

    // Step 4: search_code with detail_level: "context"
    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate_token",
                "detail_level": "context"
            }
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(
        response.error.is_none(),
        "search_code(context) should succeed"
    );
    let results = extract_results_from_response(&response);
    assert!(!results.is_empty(), "search should find validate_token");

    // Verify metadata conforms to Protocol v1
    let result = response.result.unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    let meta = payload.get("metadata").unwrap();
    assert!(
        meta.get("codecompass_protocol_version").is_some(),
        "metadata should include protocol version"
    );
    assert!(
        meta.get("freshness_status").is_some(),
        "metadata should include freshness_status"
    );
    assert!(
        meta.get("schema_status").is_some(),
        "metadata should include schema_status"
    );
}

#[test]
fn t190_new_navigation_tools_jsonrpc_e2e() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());
    let db_path = tmp.path().join("data/state.db");
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();
    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    let hierarchy_request = make_request(
        "tools/call",
        json!({
            "name": "get_symbol_hierarchy",
            "arguments": {
                "symbol_name": "authenticate",
                "path": "src/handler.rs",
                "direction": "ancestors"
            }
        }),
    );
    let hierarchy_response = handle_request_with_ctx(
        &hierarchy_request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(
        hierarchy_response.error.is_none(),
        "hierarchy tool should succeed"
    );
    let hierarchy_payload = extract_payload_from_response(&hierarchy_response);
    assert!(hierarchy_payload.get("hierarchy").unwrap().is_array());
    assert!(hierarchy_payload.get("metadata").is_some());

    let related_request = make_request(
        "tools/call",
        json!({
            "name": "find_related_symbols",
            "arguments": {
                "symbol_name": "authenticate",
                "path": "src/handler.rs",
                "scope": "file",
                "limit": 10
            }
        }),
    );
    let related_response = handle_request_with_ctx(
        &related_request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(
        related_response.error.is_none(),
        "related tool should succeed"
    );
    let related_payload = extract_payload_from_response(&related_response);
    assert!(related_payload.get("anchor").is_some());
    assert!(related_payload.get("related").unwrap().is_array());
    assert!(related_payload.get("metadata").is_some());

    let context_request = make_request(
        "tools/call",
        json!({
            "name": "get_code_context",
            "arguments": {
                "query": "validate_token",
                "strategy": "breadth",
                "max_tokens": 300,
                "language": "rust"
            }
        }),
    );
    let context_response = handle_request_with_ctx(
        &context_request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(
        context_response.error.is_none(),
        "context tool should succeed"
    );
    let context_payload = extract_payload_from_response(&context_response);
    assert!(context_payload.get("context_items").unwrap().is_array());
    assert!(
        context_payload
            .get("estimated_tokens")
            .unwrap()
            .as_u64()
            .unwrap()
            <= 300,
        "context estimated_tokens should honor max_tokens"
    );
    assert!(context_payload.get("metadata").is_some());
}

// ------------------------------------------------------------------
// T139: Backward compatibility - default detail_level is "signature"
// ------------------------------------------------------------------

#[test]
fn t139_backward_compatibility_default_detail_level() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test_project";

    // Call locate_symbol without detail_level parameter
    let request = make_request(
        "tools/call",
        json!({
            "name": "locate_symbol",
            "arguments": {
                "name": "validate_token"
            }
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(
        response.error.is_none(),
        "locate_symbol without detail_level should succeed"
    );
    let results = extract_results_from_response(&response);
    assert!(!results.is_empty(), "should find validate_token");

    let vt = results
        .iter()
        .find(|r| r.get("name").unwrap().as_str().unwrap() == "validate_token")
        .unwrap();

    // Signature-level fields should be present (default)
    assert!(
        vt.get("qualified_name").is_some(),
        "default should include qualified_name (signature level)"
    );
    assert!(
        vt.get("language").is_some(),
        "default should include language (signature level)"
    );

    // Context-only fields should NOT be present
    assert!(
        vt.get("body_preview").is_none(),
        "default should NOT include body_preview (context only)"
    );

    // Same test for search_code
    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate_token"
            }
        }),
    );
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: None,
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    assert!(
        response.error.is_none(),
        "search_code without detail_level should succeed"
    );
    let results = extract_results_from_response(&response);
    assert!(!results.is_empty(), "search should find results");

    // Verify metadata is present
    let result = response.result.unwrap();
    let content = result.get("content").unwrap().as_array().unwrap();
    let text = content[0].get("text").unwrap().as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    assert!(
        payload.get("metadata").is_some(),
        "response should include metadata for backward compatibility"
    );
}

// ------------------------------------------------------------------
// T138: Performance benchmark
// ------------------------------------------------------------------

#[test]
fn t138_performance_benchmark() {
    let tmp = tempfile::tempdir().unwrap();
    let index_set = build_fixture_index(tmp.path());

    let db_path = tmp.path().join("data/state.db");
    let conn = codecompass_state::db::open_connection(&db_path).unwrap();

    let config = Config::default();
    let workspace = Path::new("/tmp/fake-workspace");
    let project_id = "test-repo";

    // Benchmark get_file_outline: measure 10 iterations, verify p95 < 50ms
    let mut outline_times = Vec::new();
    for _ in 0..10 {
        let request = make_request(
            "tools/call",
            json!({
                "name": "get_file_outline",
                "arguments": {
                    "path": "src/auth.rs"
                }
            }),
        );
        let start = std::time::Instant::now();
        let response = handle_request_with_ctx(
            &request,
            &RequestContext {
                config: &config,
                index_set: Some(&index_set),
                schema_status: SchemaStatus::Compatible,
                compatibility_reason: None,
                conn: Some(&conn),
                workspace,
                project_id,
                prewarm_status: &test_prewarm_status(),
                server_start: &test_server_start(),
            },
        );
        let elapsed = start.elapsed();
        outline_times.push(elapsed);
        assert!(response.error.is_none(), "get_file_outline should succeed");
    }
    outline_times.sort();
    let p95_outline = outline_times[8]; // 95th percentile of 10 samples
    assert!(
        p95_outline.as_millis() < 50,
        "get_file_outline p95 should be < 50ms, got {}ms",
        p95_outline.as_millis()
    );

    // Benchmark first-query latency: search_code after prewarm
    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate_token"
            }
        }),
    );
    let start = std::time::Instant::now();
    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );
    let elapsed = start.elapsed();
    assert!(response.error.is_none(), "search_code should succeed");
    assert!(
        elapsed.as_millis() < 500,
        "first-query latency should be < 500ms, got {}ms",
        elapsed.as_millis()
    );
}

// ------------------------------------------------------------------
// Helper: extract the full JSON payload from an MCP tool response
// ------------------------------------------------------------------

fn extract_payload_from_response(response: &JsonRpcResponse) -> serde_json::Value {
    let result = response.result.as_ref().expect("result should be present");
    let content = result
        .get("content")
        .expect("result should have 'content'")
        .as_array()
        .expect("'content' should be an array");
    let text = content[0].get("text").unwrap().as_str().unwrap();
    serde_json::from_str(text).expect("text payload should be valid JSON")
}

// ------------------------------------------------------------------
// Helper: build a fixture index inside a real git repo for freshness tests
// ------------------------------------------------------------------

fn build_fixture_index_in_git_repo(
    tmp_dir: &std::path::Path,
) -> (IndexSet, rusqlite::Connection, String) {
    use codecompass_indexer::{
        languages, parser, scanner, snippet_extract, symbol_extract, writer,
    };
    use codecompass_state::{db, schema, tantivy_index::IndexSet};

    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/rust-sample");

    // Initialize a git repo in tmp_dir and commit
    let workspace = tmp_dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&workspace)
        .output()
        .unwrap();

    // Copy fixture files into workspace (recursive)
    fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let dest = dst.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir_recursive(&entry.path(), &dest);
            } else {
                std::fs::copy(entry.path(), &dest).unwrap();
            }
        }
    }
    copy_dir_recursive(&fixture_dir, &workspace);

    // Initial commit
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&workspace)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&workspace)
        .output()
        .unwrap();

    // Get the initial commit hash
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    let initial_commit = String::from_utf8(output.stdout).unwrap().trim().to_string();

    // Build index
    let data_dir = workspace.join(".codecompass/data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let index_set = IndexSet::open(&data_dir).unwrap();

    let db_path = data_dir.join("state.db");
    let conn = db::open_connection(&db_path).unwrap();
    schema::create_tables(&conn).unwrap();

    let repo = "test-repo";
    // Detect the current branch name
    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    let branch_name = String::from_utf8(branch_output.stdout)
        .unwrap()
        .trim()
        .to_string();

    let scanned = scanner::scan_directory(&workspace, 1_048_576);
    for file in &scanned {
        let source = std::fs::read_to_string(&file.path).unwrap();
        let tree = match parser::parse_file(&source, &file.language) {
            Ok(t) => t,
            Err(_) => continue,
        };

        let extracted = languages::extract_symbols(&tree, &source, &file.language);
        let symbols = symbol_extract::build_symbol_records(
            &extracted,
            repo,
            &branch_name,
            &file.relative_path,
            None,
        );
        let snippets = snippet_extract::build_snippet_records(
            &extracted,
            repo,
            &branch_name,
            &file.relative_path,
            None,
        );

        let content_hash = blake3::hash(source.as_bytes()).to_hex().to_string();
        let filename = file.path.file_name().unwrap().to_string_lossy().to_string();
        let file_record = codecompass_core::types::FileRecord {
            repo: repo.to_string(),
            r#ref: branch_name.clone(),
            commit: None,
            path: file.relative_path.clone(),
            filename,
            language: file.language.clone(),
            content_hash,
            size_bytes: source.len() as u64,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            content_head: source
                .lines()
                .take(10)
                .collect::<Vec<_>>()
                .join("\n")
                .into(),
        };

        writer::write_file_records(&index_set, &conn, &symbols, &snippets, &file_record).unwrap();
    }

    // Store branch_state with initial commit
    let branch_entry = codecompass_state::branch_state::BranchState {
        repo: repo.to_string(),
        r#ref: branch_name.clone(),
        merge_base_commit: None,
        last_indexed_commit: initial_commit,
        overlay_dir: None,
        file_count: scanned.len() as i64,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        last_accessed_at: "2026-01-01T00:00:00Z".to_string(),
    };
    codecompass_state::branch_state::upsert_branch_state(&conn, &branch_entry).unwrap();

    (index_set, conn, branch_name)
}

/// Make a new commit in the workspace to make the index stale.
fn make_workspace_stale(workspace: &std::path::Path) {
    let dummy = workspace.join("dummy.txt");
    std::fs::write(&dummy, "stale marker").unwrap();
    std::process::Command::new("git")
        .args(["add", "dummy.txt"])
        .current_dir(workspace)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "make stale"])
        .current_dir(workspace)
        .output()
        .unwrap();
}

// ------------------------------------------------------------------
// T131: balanced policy with stale index returns results + stale status
// ------------------------------------------------------------------

#[test]
fn t131_search_code_balanced_policy_stale_index() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, conn, branch_name) = build_fixture_index_in_git_repo(tmp.path());
    let workspace = tmp.path().join("workspace");

    // Make the index stale by creating a new commit
    make_workspace_stale(&workspace);

    let config = Config::default();
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate_token",
                "ref": branch_name,
                "freshness_policy": "balanced"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace: &workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(
        response.error.is_none(),
        "expected success, got error: {:?}",
        response.error
    );
    let payload = extract_payload_from_response(&response);

    // Should have results (query still runs)
    let results = payload.get("results").unwrap().as_array().unwrap();
    assert!(
        !results.is_empty(),
        "balanced policy should return results even when stale"
    );

    // Metadata should show stale
    let meta = payload.get("metadata").unwrap();
    assert_eq!(
        meta.get("freshness_status").unwrap().as_str().unwrap(),
        "stale",
        "freshness_status should be 'stale' for balanced policy with stale index"
    );
}

// ------------------------------------------------------------------
// T132: strict policy with stale index returns index_stale error
// ------------------------------------------------------------------

#[test]
fn t132_search_code_strict_policy_stale_index_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, conn, branch_name) = build_fixture_index_in_git_repo(tmp.path());
    let workspace = tmp.path().join("workspace");

    // Make the index stale
    make_workspace_stale(&workspace);

    let config = Config::default();
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate_token",
                "ref": branch_name,
                "freshness_policy": "strict"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace: &workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    // The response should be a "success" at JSON-RPC level but contain an error payload
    assert!(
        response.error.is_none(),
        "should be JSON-RPC success (error is in payload)"
    );
    let payload = extract_payload_from_response(&response);

    // Should have error object instead of results
    let error = payload
        .get("error")
        .expect("payload should have 'error' for strict+stale");
    assert_eq!(
        error.get("code").unwrap().as_str().unwrap(),
        "index_stale",
        "error code should be 'index_stale'"
    );
    assert!(
        error
            .get("message")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("stale"),
        "error message should mention stale"
    );
    let data = error.get("data").unwrap();
    assert!(
        data.get("last_indexed_commit").is_some(),
        "error data should include last_indexed_commit"
    );
    assert!(
        data.get("current_head").is_some(),
        "error data should include current_head"
    );
    assert!(
        data.get("suggestion").is_some(),
        "error data should include suggestion"
    );

    // Metadata should show stale
    let meta = payload.get("metadata").unwrap();
    assert_eq!(
        meta.get("freshness_status").unwrap().as_str().unwrap(),
        "stale"
    );
}

// ------------------------------------------------------------------
// T133: best_effort policy with stale index returns results + stale
// ------------------------------------------------------------------

#[test]
fn t133_search_code_best_effort_policy_stale_index() {
    let tmp = tempfile::tempdir().unwrap();
    let (index_set, conn, branch_name) = build_fixture_index_in_git_repo(tmp.path());
    let workspace = tmp.path().join("workspace");

    // Make the index stale
    make_workspace_stale(&workspace);

    let config = Config::default();
    let project_id = "test-repo";

    let request = make_request(
        "tools/call",
        json!({
            "name": "search_code",
            "arguments": {
                "query": "validate_token",
                "ref": branch_name,
                "freshness_policy": "best_effort"
            }
        }),
    );

    let response = handle_request_with_ctx(
        &request,
        &RequestContext {
            config: &config,
            index_set: Some(&index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
            conn: Some(&conn),
            workspace: &workspace,
            project_id,
            prewarm_status: &test_prewarm_status(),
            server_start: &test_server_start(),
        },
    );

    assert!(response.error.is_none(), "expected success");
    let payload = extract_payload_from_response(&response);

    // Should have results (best_effort always returns)
    let results = payload.get("results").unwrap().as_array().unwrap();
    assert!(
        !results.is_empty(),
        "best_effort policy should return results even when stale"
    );

    // Metadata should show stale
    let meta = payload.get("metadata").unwrap();
    assert_eq!(
        meta.get("freshness_status").unwrap().as_str().unwrap(),
        "stale",
        "freshness_status should be 'stale' for best_effort policy with stale index"
    );
}
