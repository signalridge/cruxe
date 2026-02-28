use cruxe_core::error::StateError;
use cruxe_core::types::{CallEdge, SymbolKind, SymbolRecord};
use rusqlite::{Connection, params};
use std::collections::{HashMap, HashSet, hash_map::Entry};
use tracing::debug;

use crate::import_extract::source_symbol_id_for_path;

/// Extract per-file call edges from parsed AST and resolve caller symbols by line coverage.
///
/// Callee resolution is deferred to `resolve_call_targets`.
pub fn extract_call_edges_for_file(
    tree: &tree_sitter::Tree,
    source: &str,
    language: &str,
    source_file: &str,
    symbols: &[SymbolRecord],
    repo: &str,
    ref_name: &str,
) -> Vec<CallEdge> {
    let call_sites = crate::languages::extract_call_sites(tree, source, language);
    let mut edges = Vec::new();
    for site in call_sites {
        let caller_id = resolve_caller_symbol(symbols, site.line)
            .map(|caller| caller.symbol_stable_id.clone())
            .unwrap_or_else(|| source_symbol_id_for_path(source_file));
        if site.callee_name.trim().is_empty() {
            continue;
        }
        edges.push(CallEdge {
            repo: repo.to_string(),
            ref_name: ref_name.to_string(),
            from_symbol_id: caller_id,
            to_symbol_id: None,
            to_name: Some(site.callee_name),
            edge_type: "calls".to_string(),
            confidence: site.confidence,
            source_file: source_file.to_string(),
            source_line: site.line,
        });
    }
    dedup_call_edges(edges)
}

/// Resolve `to_symbol_id` for extracted call edges using symbols indexed under `(repo, ref)`.
///
/// Unresolved edges retain `to_symbol_id = None` and keep `to_name`.
pub fn resolve_call_targets(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    edges: &mut [CallEdge],
) -> Result<(), StateError> {
    let lookup = load_symbol_lookup(conn, repo, ref_name)?;
    resolve_call_targets_with_lookup(&lookup, edges);
    Ok(())
}

/// Load symbol lookup tables for call-target resolution under `(repo, ref)`.
pub fn load_symbol_lookup(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
) -> Result<SymbolLookup, StateError> {
    SymbolLookup::load(conn, repo, ref_name)
}

/// Resolve `to_symbol_id` for call edges using a preloaded symbol lookup.
///
/// This is optimized for batch workflows where many files share the same
/// `(repo, ref)` scope and call-target resolution must avoid repeated DB scans.
pub fn resolve_call_targets_with_lookup(lookup: &SymbolLookup, edges: &mut [CallEdge]) {
    for edge in edges.iter_mut() {
        let Some(raw_target) = edge.to_name.as_ref() else {
            continue;
        };
        let normalized = normalize_target(raw_target);
        if normalized.is_empty() {
            continue;
        }
        if let Some(symbol_id) = lookup.resolve(&normalized) {
            if lookup.is_ambiguous_resolution(&normalized) {
                debug!(
                    target = %normalized,
                    resolved_symbol_id = %symbol_id,
                    "Ambiguous short-name call target resolved to deterministic first match"
                );
            }
            edge.to_symbol_id = Some(symbol_id);
            edge.to_name = None;
        } else {
            edge.to_symbol_id = None;
            edge.to_name = Some(normalized);
        }
    }
}

/// Deduplicate call edges by caller/callee/call-site tuple.
pub fn dedup_call_edges(edges: Vec<CallEdge>) -> Vec<CallEdge> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for edge in edges {
        let key = (
            edge.from_symbol_id.clone(),
            edge.to_symbol_id.clone(),
            edge.to_name.clone(),
            edge.source_file.clone(),
            edge.source_line,
            edge.edge_type.clone(),
        );
        if seen.insert(key) {
            deduped.push(edge);
        }
    }
    deduped
}

fn resolve_caller_symbol(symbols: &[SymbolRecord], line: u32) -> Option<&SymbolRecord> {
    symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
        .filter(|symbol| line >= symbol.line_start && line <= symbol.line_end)
        .min_by_key(|symbol| symbol.line_end.saturating_sub(symbol.line_start))
}

fn normalize_target(target: &str) -> String {
    let normalized = target
        .trim()
        .trim_start_matches("new ")
        .trim_start_matches("await ")
        .trim()
        .trim_end_matches('?')
        .trim_end_matches('!')
        .trim()
        .to_string();
    strip_rust_turbofish_segments(&normalized)
}

fn strip_rust_turbofish_segments(target: &str) -> String {
    let chars: Vec<char> = target.chars().collect();
    let mut result = String::with_capacity(target.len());
    let mut idx = 0usize;

    while idx < chars.len() {
        if idx + 2 < chars.len()
            && chars[idx] == ':'
            && chars[idx + 1] == ':'
            && chars[idx + 2] == '<'
        {
            let mut cursor = idx + 3;
            let mut depth = 1usize;
            while cursor < chars.len() && depth > 0 {
                match chars[cursor] {
                    '<' => depth += 1,
                    '>' => depth = depth.saturating_sub(1),
                    _ => {}
                }
                cursor += 1;
            }
            if depth == 0 {
                idx = cursor;
                continue;
            }
        }
        result.push(chars[idx]);
        idx += 1;
    }

    result
}

fn last_segment(value: &str) -> &str {
    let dot = value.rsplit('.').next().unwrap_or(value);
    dot.rsplit("::").next().unwrap_or(dot)
}

pub struct SymbolLookup {
    by_qualified: HashMap<String, String>,
    by_name: HashMap<String, String>,
    ambiguous_short_names: HashSet<String>,
}

impl SymbolLookup {
    fn load(conn: &Connection, repo: &str, ref_name: &str) -> Result<Self, StateError> {
        let mut stmt = conn
            .prepare(
                "SELECT symbol_stable_id, name, qualified_name
                 FROM symbol_relations
                 WHERE repo = ?1 AND \"ref\" = ?2
                 ORDER BY qualified_name, path, line_start, symbol_stable_id",
            )
            .map_err(StateError::sqlite)?;
        let rows = stmt
            .query_map(params![repo, ref_name], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(StateError::sqlite)?;

        let mut by_qualified = HashMap::new();
        let mut by_name = HashMap::new();
        let mut ambiguous_short_names = HashSet::new();
        for row in rows {
            let (symbol_stable_id, name, qualified_name) = row.map_err(StateError::sqlite)?;
            by_qualified
                .entry(qualified_name.clone())
                .or_insert_with(|| symbol_stable_id.clone());
            let tail = last_segment(&qualified_name).to_string();
            track_short_name_mapping(
                &mut by_name,
                &mut ambiguous_short_names,
                tail,
                symbol_stable_id.as_str(),
            );
            track_short_name_mapping(
                &mut by_name,
                &mut ambiguous_short_names,
                name,
                symbol_stable_id.as_str(),
            );
        }

        Ok(Self {
            by_qualified,
            by_name,
            ambiguous_short_names,
        })
    }

    fn resolve(&self, target: &str) -> Option<String> {
        if let Some(id) = self.by_qualified.get(target) {
            return Some(id.clone());
        }
        let tail = last_segment(target);
        self.by_name.get(tail).cloned()
    }

    fn is_ambiguous_resolution(&self, target: &str) -> bool {
        if self.by_qualified.contains_key(target) {
            return false;
        }
        let tail = last_segment(target);
        self.ambiguous_short_names.contains(tail)
    }
}

fn track_short_name_mapping(
    by_name: &mut HashMap<String, String>,
    ambiguous_short_names: &mut HashSet<String>,
    key: String,
    symbol_stable_id: &str,
) {
    match by_name.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(symbol_stable_id.to_string());
        }
        Entry::Occupied(entry) => {
            if entry.get() != symbol_stable_id {
                ambiguous_short_names.insert(entry.key().clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use cruxe_core::types::Project;
    use cruxe_state::{db, project, schema, symbols};

    fn setup() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        (tmp, conn)
    }

    fn symbol(
        repo: &str,
        ref_name: &str,
        symbol_stable_id: &str,
        name: &str,
        qualified_name: &str,
        line_start: u32,
        line_end: u32,
    ) -> SymbolRecord {
        SymbolRecord {
            repo: repo.to_string(),
            r#ref: ref_name.to_string(),
            commit: None,
            path: "src/lib.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: format!("sym::{name}"),
            symbol_stable_id: symbol_stable_id.to_string(),
            name: name.to_string(),
            qualified_name: qualified_name.to_string(),
            kind: SymbolKind::Function,
            signature: Some(format!("fn {name}()")),
            line_start,
            line_end,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: None,
        }
    }

    #[test]
    fn extract_call_edges_resolves_caller_by_line() {
        let source = r#"
fn handler() {
    validate_token();
    client.process();
}

fn validate_token() {}
"#;
        let tree = parser::parse_file(source, "rust").unwrap();
        let symbols = vec![
            symbol("repo", "main", "stable-handler", "handler", "handler", 2, 5),
            symbol(
                "repo",
                "main",
                "stable-validate",
                "validate_token",
                "validate_token",
                7,
                7,
            ),
        ];

        let edges = extract_call_edges_for_file(
            &tree,
            source,
            "rust",
            "src/lib.rs",
            &symbols,
            "repo",
            "main",
        );
        assert_eq!(edges.len(), 2);
        assert!(
            edges
                .iter()
                .all(|edge| edge.from_symbol_id == "stable-handler")
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge.to_name.as_deref() == Some("validate_token"))
        );
    }

    #[test]
    fn extract_call_edges_uses_file_source_for_module_scope_calls() {
        let source = r#"
const db = createPool();

function handler() {
    validateToken();
}
"#;
        let tree = parser::parse_file(source, "typescript").unwrap();
        let symbols = vec![symbol(
            "repo",
            "main",
            "stable-handler",
            "handler",
            "handler",
            4,
            6,
        )];

        let edges = extract_call_edges_for_file(
            &tree,
            source,
            "typescript",
            "src/lib.rs",
            &symbols,
            "repo",
            "main",
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge.from_symbol_id == "file::src/lib.rs"),
            "expected module-scope call to fall back to file::<path> caller"
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge.from_symbol_id == "stable-handler"),
            "expected function-scoped call to retain function caller"
        );
    }

    #[test]
    fn resolve_call_targets_prefers_qualified_then_name() {
        let (tmp, conn) = setup();
        let repo_root = tmp.path().join("workspace");
        std::fs::create_dir_all(&repo_root).unwrap();
        project::create_project(
            &conn,
            &Project {
                project_id: "repo".to_string(),
                repo_root: repo_root.to_string_lossy().to_string(),
                display_name: Some("repo".to_string()),
                default_ref: "main".to_string(),
                vcs_mode: false,
                schema_version: 1,
                parser_version: 1,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let target = symbol(
            "repo",
            "main",
            "stable-validate",
            "validate_token",
            "auth::validate_token",
            1,
            3,
        );
        symbols::insert_symbol(&conn, &target).unwrap();

        let mut edges = vec![
            CallEdge {
                repo: "repo".to_string(),
                ref_name: "main".to_string(),
                from_symbol_id: "stable-handler".to_string(),
                to_symbol_id: None,
                to_name: Some("auth::validate_token".to_string()),
                edge_type: "calls".to_string(),
                confidence: "static".to_string(),
                source_file: "src/lib.rs".to_string(),
                source_line: 10,
            },
            CallEdge {
                repo: "repo".to_string(),
                ref_name: "main".to_string(),
                from_symbol_id: "stable-handler".to_string(),
                to_symbol_id: None,
                to_name: Some("external_call".to_string()),
                edge_type: "calls".to_string(),
                confidence: "heuristic".to_string(),
                source_file: "src/lib.rs".to_string(),
                source_line: 11,
            },
        ];
        let lookup = load_symbol_lookup(&conn, "repo", "main").unwrap();
        resolve_call_targets_with_lookup(&lookup, &mut edges);

        assert_eq!(edges[0].to_symbol_id.as_deref(), Some("stable-validate"));
        assert_eq!(edges[0].to_name, None);
        assert_eq!(edges[1].to_symbol_id, None);
        assert_eq!(edges[1].to_name.as_deref(), Some("external_call"));
    }

    #[test]
    fn resolve_call_targets_marks_short_name_collisions_as_ambiguous() {
        let (tmp, conn) = setup();
        let repo_root = tmp.path().join("workspace");
        std::fs::create_dir_all(&repo_root).unwrap();
        project::create_project(
            &conn,
            &Project {
                project_id: "repo".to_string(),
                repo_root: repo_root.to_string_lossy().to_string(),
                display_name: Some("repo".to_string()),
                default_ref: "main".to_string(),
                vcs_mode: false,
                schema_version: 1,
                parser_version: 1,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        symbols::insert_symbol(
            &conn,
            &symbol(
                "repo",
                "main",
                "stable-a",
                "validate",
                "auth::validate",
                1,
                2,
            ),
        )
        .unwrap();
        symbols::insert_symbol(
            &conn,
            &symbol(
                "repo",
                "main",
                "stable-b",
                "validate",
                "json::validate",
                10,
                11,
            ),
        )
        .unwrap();

        let lookup = load_symbol_lookup(&conn, "repo", "main").unwrap();
        assert!(lookup.is_ambiguous_resolution("validate"));
        assert!(!lookup.is_ambiguous_resolution("auth::validate"));
    }

    #[test]
    fn resolve_call_targets_does_not_mark_single_symbol_short_name_as_ambiguous() {
        let (tmp, conn) = setup();
        let repo_root = tmp.path().join("workspace");
        std::fs::create_dir_all(&repo_root).unwrap();
        project::create_project(
            &conn,
            &Project {
                project_id: "repo".to_string(),
                repo_root: repo_root.to_string_lossy().to_string(),
                display_name: Some("repo".to_string()),
                default_ref: "main".to_string(),
                vcs_mode: false,
                schema_version: 1,
                parser_version: 1,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        symbols::insert_symbol(
            &conn,
            &symbol(
                "repo",
                "main",
                "stable-auth-validate",
                "validate",
                "auth::validate",
                1,
                2,
            ),
        )
        .unwrap();

        let lookup = load_symbol_lookup(&conn, "repo", "main").unwrap();
        assert!(
            !lookup.is_ambiguous_resolution("validate"),
            "single symbol short-name mapping should not be treated as ambiguous"
        );
    }

    #[test]
    fn resolve_call_targets_uses_deterministic_lookup_order_for_name_collisions() {
        let (tmp, conn) = setup();
        let repo_root = tmp.path().join("workspace");
        std::fs::create_dir_all(&repo_root).unwrap();
        project::create_project(
            &conn,
            &Project {
                project_id: "repo".to_string(),
                repo_root: repo_root.to_string_lossy().to_string(),
                display_name: Some("repo".to_string()),
                default_ref: "main".to_string(),
                vcs_mode: false,
                schema_version: 1,
                parser_version: 1,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let mut z_target = symbol(
            "repo",
            "main",
            "stable-z",
            "validate",
            "z::validate",
            20,
            22,
        );
        z_target.path = "src/z.rs".to_string();
        let mut a_target = symbol(
            "repo",
            "main",
            "stable-a",
            "validate",
            "a::validate",
            10,
            12,
        );
        a_target.path = "src/a.rs".to_string();

        // Insert out of lexical order to verify lookup ordering is deterministic.
        symbols::insert_symbol(&conn, &z_target).unwrap();
        symbols::insert_symbol(&conn, &a_target).unwrap();

        let lookup = load_symbol_lookup(&conn, "repo", "main").unwrap();
        let mut edges = vec![
            CallEdge {
                repo: "repo".to_string(),
                ref_name: "main".to_string(),
                from_symbol_id: "stable-handler".to_string(),
                to_symbol_id: None,
                to_name: Some("validate".to_string()),
                edge_type: "calls".to_string(),
                confidence: "heuristic".to_string(),
                source_file: "src/lib.rs".to_string(),
                source_line: 30,
            },
            CallEdge {
                repo: "repo".to_string(),
                ref_name: "main".to_string(),
                from_symbol_id: "stable-handler".to_string(),
                to_symbol_id: None,
                to_name: Some("z::validate".to_string()),
                edge_type: "calls".to_string(),
                confidence: "static".to_string(),
                source_file: "src/lib.rs".to_string(),
                source_line: 31,
            },
        ];

        resolve_call_targets_with_lookup(&lookup, &mut edges);
        assert_eq!(edges[0].to_symbol_id.as_deref(), Some("stable-a"));
        assert_eq!(edges[0].to_name, None);
        assert_eq!(edges[1].to_symbol_id.as_deref(), Some("stable-z"));
        assert_eq!(edges[1].to_name, None);
    }

    #[test]
    fn dedup_call_edges_removes_duplicates() {
        let edge = CallEdge {
            repo: "repo".to_string(),
            ref_name: "main".to_string(),
            from_symbol_id: "stable-handler".to_string(),
            to_symbol_id: Some("stable-validate".to_string()),
            to_name: None,
            edge_type: "calls".to_string(),
            confidence: "static".to_string(),
            source_file: "src/lib.rs".to_string(),
            source_line: 10,
        };
        let deduped = dedup_call_edges(vec![edge.clone(), edge]);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn normalize_target_strips_rust_turbofish_segments() {
        assert_eq!(
            normalize_target("auth::validate_token::<Claims>"),
            "auth::validate_token"
        );
        assert_eq!(normalize_target("Vec::<u8>::new"), "Vec::new");
        assert_eq!(
            normalize_target("outer::call::<Vec<Result<T, E>>>::run"),
            "outer::call::run"
        );
    }

    #[test]
    fn resolve_call_targets_matches_turbofish_target() {
        let (tmp, conn) = setup();
        let repo_root = tmp.path().join("workspace");
        std::fs::create_dir_all(&repo_root).unwrap();
        project::create_project(
            &conn,
            &Project {
                project_id: "repo".to_string(),
                repo_root: repo_root.to_string_lossy().to_string(),
                display_name: Some("repo".to_string()),
                default_ref: "main".to_string(),
                vcs_mode: false,
                schema_version: 1,
                parser_version: 1,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let target = symbol(
            "repo",
            "main",
            "stable-validate",
            "validate_token",
            "auth::validate_token",
            1,
            3,
        );
        symbols::insert_symbol(&conn, &target).unwrap();

        let lookup = load_symbol_lookup(&conn, "repo", "main").unwrap();
        let mut edges = vec![CallEdge {
            repo: "repo".to_string(),
            ref_name: "main".to_string(),
            from_symbol_id: "stable-handler".to_string(),
            to_symbol_id: None,
            to_name: Some("auth::validate_token::<Claims>".to_string()),
            edge_type: "calls".to_string(),
            confidence: "static".to_string(),
            source_file: "src/lib.rs".to_string(),
            source_line: 10,
        }];

        resolve_call_targets_with_lookup(&lookup, &mut edges);
        assert_eq!(edges[0].to_symbol_id.as_deref(), Some("stable-validate"));
        assert_eq!(edges[0].to_name, None);
    }

    #[test]
    fn extract_call_edges_uses_file_caller_for_module_scope_calls() {
        let source = "const x = helper();\nfunction helper() { return 1; }\n";
        let tree = parser::parse_file(source, "typescript").unwrap();
        let symbols = vec![symbol(
            "repo",
            "main",
            "stable-helper",
            "helper",
            "helper",
            2,
            2,
        )];

        let edges = extract_call_edges_for_file(
            &tree,
            source,
            "typescript",
            "src/main.ts",
            &symbols,
            "repo",
            "main",
        );
        assert!(
            edges
                .iter()
                .any(|edge| edge.from_symbol_id == "file::src/main.ts"),
            "module-scope call should fall back to file::<path> as caller"
        );
    }
}
