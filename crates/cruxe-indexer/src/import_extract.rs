use crate::languages;
use cruxe_core::edge_confidence::{
    EDGE_PROVIDER_IMPORT_RESOLVER, RESOLUTION_EXTERNAL_REFERENCE, RESOLUTION_RESOLVED_INTERNAL,
    RESOLUTION_UNRESOLVED, assign_edge_confidence, looks_external_reference,
};
use cruxe_core::error::StateError;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Raw import extracted from source code before symbol resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawImport {
    pub source_qualified_name: String,
    pub target_qualified_name: String,
    pub target_name: String,
    pub import_line: u32,
}

/// Resolved import edge payload with nullable `to_symbol_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedImportEdge {
    pub repo: String,
    pub ref_name: String,
    pub from_symbol_id: String,
    pub to_symbol_id: Option<String>,
    pub to_name: Option<String>,
    pub edge_type: String,
    pub confidence: String,
    pub edge_provider: String,
    pub resolution_outcome: String,
    pub confidence_weight: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveOutcome {
    ResolvedInternal,
    ExternalReference,
    Unresolved,
}

impl ResolveOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResolvedInternal => RESOLUTION_RESOLVED_INTERNAL,
            Self::ExternalReference => RESOLUTION_EXTERNAL_REFERENCE,
            Self::Unresolved => RESOLUTION_UNRESOLVED,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportResolutionStats {
    pub attempts: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub by_provider: HashMap<String, usize>,
}

impl ImportResolutionStats {
    pub fn unresolved_rate(&self) -> f64 {
        if self.attempts == 0 {
            return 0.0;
        }
        self.unresolved as f64 / self.attempts as f64
    }

    pub fn import_resolution_rate(&self) -> f64 {
        if self.attempts == 0 {
            return 0.0;
        }
        self.resolved as f64 / self.attempts as f64
    }
}

#[derive(Debug, Clone)]
pub struct ImportResolution {
    pub to_symbol_id: Option<String>,
    pub outcome: ResolveOutcome,
    pub provider: &'static str,
}

pub trait ImportResolverProvider {
    fn provider_name(&self) -> &'static str;

    fn resolve(
        &self,
        conn: &Connection,
        repo: &str,
        ref_name: &str,
        raw: &RawImport,
    ) -> Result<ImportResolution, StateError>;
}

#[derive(Debug, Default)]
pub struct GenericHeuristicResolverProvider;

impl ImportResolverProvider for GenericHeuristicResolverProvider {
    fn provider_name(&self) -> &'static str {
        "generic_heuristic"
    }

    fn resolve(
        &self,
        conn: &Connection,
        repo: &str,
        ref_name: &str,
        raw: &RawImport,
    ) -> Result<ImportResolution, StateError> {
        resolve_target_symbol_stable_id_generic(conn, repo, ref_name, raw, self.provider_name())
    }
}

/// Deterministic pseudo symbol ID for file-scoped import edges.
pub fn source_symbol_id_for_path(path: &str) -> String {
    format!("file::{}", path)
}

/// Extract raw imports by language.
pub fn extract_imports(
    tree: &tree_sitter::Tree,
    source: &str,
    language: &str,
    source_path: &str,
) -> Vec<RawImport> {
    match language {
        "rust" => languages::rust::extract_imports(tree, source, source_path),
        "typescript" => languages::typescript::extract_imports(tree, source, source_path),
        "python" => languages::python::extract_imports(tree, source, source_path),
        "go" => languages::go::extract_imports(tree, source, source_path),
        _ => Vec::new(),
    }
}

/// Resolve raw imports to edge records with nullable `to_symbol_id`.
pub fn resolve_imports(
    conn: &Connection,
    raw_imports: Vec<RawImport>,
    repo: &str,
    ref_name: &str,
) -> Result<Vec<ResolvedImportEdge>, StateError> {
    let (edges, stats) = resolve_imports_with_stats(conn, raw_imports, repo, ref_name)?;
    tracing::info!(
        attempts = stats.attempts,
        resolved = stats.resolved,
        unresolved = stats.unresolved,
        unresolved_rate = stats.unresolved_rate(),
        import_resolution_rate = stats.import_resolution_rate(),
        by_provider = ?stats.by_provider,
        "import resolution run stats"
    );
    Ok(edges)
}

pub fn resolve_imports_with_stats(
    conn: &Connection,
    raw_imports: Vec<RawImport>,
    repo: &str,
    ref_name: &str,
) -> Result<(Vec<ResolvedImportEdge>, ImportResolutionStats), StateError> {
    let mut edges = Vec::new();
    let mut seen = HashSet::new();
    let mut stats = ImportResolutionStats::default();
    let generic = GenericHeuristicResolverProvider;
    let providers: [&dyn ImportResolverProvider; 1] = [&generic];

    for raw in raw_imports {
        stats.attempts += 1;
        let resolution = resolve_with_provider_chain(conn, repo, ref_name, &raw, &providers)?;
        match resolution.outcome {
            ResolveOutcome::ResolvedInternal if resolution.to_symbol_id.is_some() => {
                stats.resolved += 1;
            }
            ResolveOutcome::Unresolved => {
                stats.unresolved += 1;
            }
            ResolveOutcome::ExternalReference | ResolveOutcome::ResolvedInternal => {}
        }
        *stats
            .by_provider
            .entry(resolution.provider.to_string())
            .or_insert(0) += 1;

        let unresolved_name = if resolution.to_symbol_id.is_none() {
            Some(if raw.target_name.trim().is_empty() {
                raw.target_qualified_name.clone()
            } else {
                raw.target_name.clone()
            })
        } else {
            None
        };
        let confidence = assign_edge_confidence(
            Some(EDGE_PROVIDER_IMPORT_RESOLVER),
            Some("imports"),
            Some(resolution.outcome.as_str()),
            resolution.to_symbol_id.as_deref(),
            unresolved_name.as_deref(),
            None,
        );

        let edge = ResolvedImportEdge {
            repo: repo.to_string(),
            ref_name: ref_name.to_string(),
            from_symbol_id: raw.source_qualified_name,
            to_symbol_id: resolution.to_symbol_id,
            to_name: unresolved_name,
            edge_type: "imports".to_string(),
            confidence: confidence.bucket,
            edge_provider: confidence.provider,
            resolution_outcome: confidence.outcome,
            confidence_weight: confidence.weight,
        };

        let dedupe_key = (
            edge.repo.clone(),
            edge.ref_name.clone(),
            edge.from_symbol_id.clone(),
            edge.to_symbol_id.clone(),
            edge.to_name.clone(),
            edge.edge_type.clone(),
            edge.confidence.clone(),
            edge.edge_provider.clone(),
            edge.resolution_outcome.clone(),
        );
        if seen.insert(dedupe_key) {
            edges.push(edge);
        }
    }

    Ok((edges, stats))
}

fn resolve_with_provider_chain(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    raw: &RawImport,
    providers: &[&dyn ImportResolverProvider],
) -> Result<ImportResolution, StateError> {
    let mut last_error: Option<StateError> = None;
    let mut last_unresolved: Option<ImportResolution> = None;
    for provider in providers {
        match provider.resolve(conn, repo, ref_name, raw) {
            Ok(resolution) => {
                if resolution.to_symbol_id.is_some() {
                    return Ok(resolution);
                }
                last_unresolved = Some(resolution);
            }
            Err(err) => {
                // Fail-soft provider chain: try next provider before bubbling up.
                tracing::warn!(
                    provider = provider.provider_name(),
                    error = %err,
                    "import resolver provider failed; continuing to next provider"
                );
                last_error = Some(err);
            }
        }
    }
    if let Some(unresolved) = last_unresolved {
        return Ok(unresolved);
    }

    if let Some(err) = last_error {
        return Err(err);
    }
    Ok(ImportResolution {
        to_symbol_id: None,
        outcome: ResolveOutcome::Unresolved,
        provider: "none",
    })
}

fn resolve_target_symbol_stable_id_generic(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    raw: &RawImport,
    provider: &'static str,
) -> Result<ImportResolution, StateError> {
    if !raw.target_qualified_name.is_empty() {
        let mut stmt = conn
            .prepare(
                "SELECT symbol_stable_id FROM symbol_relations
                 WHERE repo = ?1 AND \"ref\" = ?2 AND qualified_name = ?3
                 LIMIT 1",
            )
            .map_err(StateError::sqlite)?;
        let exact = stmt
            .query_row(params![repo, ref_name, raw.target_qualified_name], |row| {
                row.get::<_, String>(0)
            })
            .ok();
        if exact.is_some() {
            return Ok(ImportResolution {
                to_symbol_id: exact,
                outcome: ResolveOutcome::ResolvedInternal,
                provider,
            });
        }
    }

    if !raw.target_name.is_empty() {
        let mut stmt = conn
            .prepare(
                "SELECT symbol_stable_id FROM symbol_relations
                 WHERE repo = ?1 AND \"ref\" = ?2 AND name = ?3
                 ORDER BY line_start
                 LIMIT 1",
            )
            .map_err(StateError::sqlite)?;
        let by_name = stmt
            .query_row(params![repo, ref_name, raw.target_name], |row| {
                row.get::<_, String>(0)
            })
            .ok();
        if by_name.is_some() {
            return Ok(ImportResolution {
                to_symbol_id: by_name,
                outcome: ResolveOutcome::ResolvedInternal,
                provider,
            });
        }
    }

    if let Some(importing_file) = raw.source_qualified_name.strip_prefix("file::") {
        let module_spec = module_spec_for_lookup(raw);
        if let Some(resolved_path) = resolve_import_path(
            importing_file,
            module_spec.as_str(),
            infer_language_from_path(importing_file),
        ) {
            if !file_exists_in_manifest(conn, repo, ref_name, &resolved_path)? {
                return Ok(ImportResolution {
                    to_symbol_id: None,
                    outcome: ResolveOutcome::ExternalReference,
                    provider,
                });
            }
            let mut stmt = conn
                .prepare(
                    "SELECT symbol_stable_id
                     FROM symbol_relations
                     WHERE repo = ?1 AND \"ref\" = ?2 AND path = ?3
                       AND (qualified_name = ?4 OR name = ?5)
                     ORDER BY line_start
                     LIMIT 1",
                )
                .map_err(StateError::sqlite)?;
            let from_resolved_path = stmt
                .query_row(
                    params![
                        repo,
                        ref_name,
                        resolved_path,
                        raw.target_qualified_name,
                        raw.target_name
                    ],
                    |row| row.get::<_, String>(0),
                )
                .ok();
            if from_resolved_path.is_some() {
                return Ok(ImportResolution {
                    to_symbol_id: from_resolved_path,
                    outcome: ResolveOutcome::ResolvedInternal,
                    provider,
                });
            }
            return Ok(ImportResolution {
                to_symbol_id: None,
                outcome: ResolveOutcome::Unresolved,
                provider,
            });
        }
    }

    let outcome = if looks_external_reference(raw.target_qualified_name.as_str()) {
        ResolveOutcome::ExternalReference
    } else {
        ResolveOutcome::Unresolved
    };
    Ok(ImportResolution {
        to_symbol_id: None,
        outcome,
        provider,
    })
}

fn file_exists_in_manifest(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    path: &str,
) -> Result<bool, StateError> {
    let exists: i64 = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM file_manifest
                WHERE repo = ?1 AND \"ref\" = ?2 AND path = ?3
                LIMIT 1
            )",
            params![repo, ref_name, path],
            |row| row.get(0),
        )
        .map_err(StateError::sqlite)?;
    Ok(exists == 1)
}

fn module_spec_for_lookup(raw: &RawImport) -> String {
    if raw.target_qualified_name.contains("::") {
        return raw
            .target_qualified_name
            .split("::")
            .next()
            .unwrap_or(raw.target_qualified_name.as_str())
            .trim_end_matches("::*")
            .to_string();
    }

    if raw.target_qualified_name.contains('.') {
        let mut parts: Vec<&str> = raw.target_qualified_name.split('.').collect();
        if parts.len() > 1 {
            parts.pop();
            return parts.join(".");
        }
    }

    raw.target_qualified_name.clone()
}

fn infer_language_from_path(path: &str) -> &str {
    if path.ends_with(".rs") {
        "rust"
    } else if path.ends_with(".go") {
        "go"
    } else if path.ends_with(".py") {
        "python"
    } else if path.ends_with(".ts")
        || path.ends_with(".tsx")
        || path.ends_with(".js")
        || path.ends_with(".jsx")
    {
        "typescript"
    } else {
        ""
    }
}

pub fn resolve_import_path(
    importing_file: &str,
    module_spec: &str,
    language: &str,
) -> Option<String> {
    let importing_dir = Path::new(importing_file)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    match language {
        "typescript" => {
            let module_path = Path::new(module_spec);
            let base = if module_path.is_absolute() {
                module_path.to_path_buf()
            } else {
                normalize_path(importing_dir.join(module_path))
            };
            for candidate in [
                base.with_extension("ts"),
                base.with_extension("tsx"),
                base.join("index.ts"),
            ] {
                if candidate.exists() {
                    return Some(candidate.to_string_lossy().replace('\\', "/"));
                }
            }
            Some(base.to_string_lossy().replace('\\', "/"))
        }
        "rust" => {
            let normalized = module_spec.trim();
            let mut parent = importing_dir.to_path_buf();
            let mut parts: Vec<&str> = normalized.split("::").collect();
            while parts.first() == Some(&"super") {
                parts.remove(0);
                parent = parent
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf();
            }
            while parts.first() == Some(&"crate") {
                parts.remove(0);
            }
            let module = parts.first().copied().unwrap_or("");
            if module.is_empty() {
                return None;
            }
            let file_candidate = normalize_path(parent.join(format!("{module}.rs")));
            if file_candidate.exists() {
                return Some(file_candidate.to_string_lossy().replace('\\', "/"));
            }
            let mod_candidate = normalize_path(parent.join(module).join("mod.rs"));
            if mod_candidate.exists() {
                return Some(mod_candidate.to_string_lossy().replace('\\', "/"));
            }
            Some(file_candidate.to_string_lossy().replace('\\', "/"))
        }
        "python" => {
            let module = module_spec.trim_start_matches('.');
            let dotted = module.replace('.', "/");
            let py_candidate = normalize_path(importing_dir.join(format!("{dotted}.py")));
            if py_candidate.exists() {
                return Some(py_candidate.to_string_lossy().replace('\\', "/"));
            }
            let init_candidate = normalize_path(importing_dir.join(dotted).join("__init__.py"));
            if init_candidate.exists() {
                return Some(init_candidate.to_string_lossy().replace('\\', "/"));
            }
            Some(py_candidate.to_string_lossy().replace('\\', "/"))
        }
        "go" => None,
        _ => None,
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(seg) => normalized.push(seg),
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                normalized.push(component.as_os_str())
            }
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use cruxe_core::types::{SymbolKind, SymbolRecord};
    use cruxe_state::{db, schema, symbols};
    use tempfile::tempdir;

    fn setup_test_db() -> Connection {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    fn insert_symbol(conn: &Connection, name: &str, qualified_name: &str, stable_id: &str) {
        let record = SymbolRecord {
            repo: "my-repo".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/auth.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: format!("sym_{name}"),
            symbol_stable_id: stable_id.to_string(),
            name: name.to_string(),
            qualified_name: qualified_name.to_string(),
            kind: SymbolKind::Struct,
            signature: None,
            line_start: 1,
            line_end: 10,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: None,
        };
        symbols::insert_symbol(conn, &record).unwrap();
    }

    struct AlwaysUnresolvedProvider;
    impl ImportResolverProvider for AlwaysUnresolvedProvider {
        fn provider_name(&self) -> &'static str {
            "always_unresolved"
        }

        fn resolve(
            &self,
            _conn: &Connection,
            _repo: &str,
            _ref_name: &str,
            _raw: &RawImport,
        ) -> Result<ImportResolution, StateError> {
            Ok(ImportResolution {
                to_symbol_id: None,
                outcome: ResolveOutcome::Unresolved,
                provider: self.provider_name(),
            })
        }
    }

    struct NameMatchProvider;
    impl ImportResolverProvider for NameMatchProvider {
        fn provider_name(&self) -> &'static str {
            "name_match"
        }

        fn resolve(
            &self,
            _conn: &Connection,
            _repo: &str,
            _ref_name: &str,
            raw: &RawImport,
        ) -> Result<ImportResolution, StateError> {
            if raw.target_name == "Claims" {
                Ok(ImportResolution {
                    to_symbol_id: Some("stable_claims".to_string()),
                    outcome: ResolveOutcome::ResolvedInternal,
                    provider: self.provider_name(),
                })
            } else {
                Ok(ImportResolution {
                    to_symbol_id: None,
                    outcome: ResolveOutcome::Unresolved,
                    provider: self.provider_name(),
                })
            }
        }
    }

    struct ErrorProvider;
    impl ImportResolverProvider for ErrorProvider {
        fn provider_name(&self) -> &'static str {
            "error_provider"
        }

        fn resolve(
            &self,
            _conn: &Connection,
            _repo: &str,
            _ref_name: &str,
            _raw: &RawImport,
        ) -> Result<ImportResolution, StateError> {
            Err(StateError::external("provider_failed"))
        }
    }

    #[test]
    fn resolve_imports_prefers_exact_qualified_name() {
        let conn = setup_test_db();
        insert_symbol(&conn, "Claims", "auth::Claims", "stable_claims");

        let imports = vec![RawImport {
            source_qualified_name: source_symbol_id_for_path("src/lib.rs"),
            target_qualified_name: "auth::Claims".to_string(),
            target_name: "Claims".to_string(),
            import_line: 3,
        }];

        let edges = resolve_imports(&conn, imports, "my-repo", "main").unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to_symbol_id.as_deref(), Some("stable_claims"));
        assert!(edges[0].to_name.is_none());
        assert_eq!(edges[0].confidence, "high");
        assert_eq!(edges[0].edge_provider, "import_resolver");
        assert_eq!(edges[0].resolution_outcome, "resolved_internal");
        assert!((edges[0].confidence_weight - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn provider_chain_uses_precedence_and_fail_soft_behavior() {
        let conn = setup_test_db();
        let raw = RawImport {
            source_qualified_name: source_symbol_id_for_path("src/lib.rs"),
            target_qualified_name: "auth::Claims".to_string(),
            target_name: "Claims".to_string(),
            import_line: 1,
        };
        let p1 = AlwaysUnresolvedProvider;
        let p2 = NameMatchProvider;
        let providers: [&dyn ImportResolverProvider; 2] = [&p1, &p2];
        let resolved =
            resolve_with_provider_chain(&conn, "my-repo", "main", &raw, &providers).unwrap();
        assert_eq!(resolved.provider, "name_match");
        assert_eq!(resolved.to_symbol_id.as_deref(), Some("stable_claims"));

        let p_err = ErrorProvider;
        let p_name = NameMatchProvider;
        let providers: [&dyn ImportResolverProvider; 2] = [&p_err, &p_name];
        let resolved =
            resolve_with_provider_chain(&conn, "my-repo", "main", &raw, &providers).unwrap();
        assert_eq!(resolved.provider, "name_match");
        assert_eq!(resolved.outcome, ResolveOutcome::ResolvedInternal);
    }

    #[test]
    fn resolve_imports_with_stats_emits_resolution_rates() {
        let conn = setup_test_db();
        insert_symbol(&conn, "Claims", "auth::Claims", "stable_claims");
        let imports = vec![
            RawImport {
                source_qualified_name: source_symbol_id_for_path("src/lib.rs"),
                target_qualified_name: "auth::Claims".to_string(),
                target_name: "Claims".to_string(),
                import_line: 3,
            },
            RawImport {
                source_qualified_name: source_symbol_id_for_path("src/lib.rs"),
                target_qualified_name: "missing::Thing".to_string(),
                target_name: "Thing".to_string(),
                import_line: 4,
            },
        ];
        let (_edges, stats) =
            resolve_imports_with_stats(&conn, imports, "my-repo", "main").unwrap();
        assert_eq!(stats.attempts, 2);
        assert_eq!(stats.resolved, 1);
        assert_eq!(
            stats.unresolved, 0,
            "external references should not inflate unresolved import metrics"
        );
        assert!((stats.import_resolution_rate() - 0.5).abs() < f64::EPSILON);
        assert!((stats.unresolved_rate() - 0.0).abs() < f64::EPSILON);
        assert_eq!(
            stats
                .by_provider
                .get("generic_heuristic")
                .copied()
                .unwrap_or(0),
            2
        );
    }

    #[test]
    fn resolve_imports_fallbacks_to_name() {
        let conn = setup_test_db();
        insert_symbol(&conn, "Claims", "Claims", "stable_claims_by_name");

        let imports = vec![RawImport {
            source_qualified_name: source_symbol_id_for_path("src/lib.rs"),
            target_qualified_name: "auth::Claims".to_string(),
            target_name: "Claims".to_string(),
            import_line: 3,
        }];

        let edges = resolve_imports(&conn, imports, "my-repo", "main").unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            edges[0].to_symbol_id.as_deref(),
            Some("stable_claims_by_name")
        );
        assert!(edges[0].to_name.is_none());
        assert_eq!(edges[0].confidence, "high");
        assert_eq!(edges[0].edge_provider, "import_resolver");
        assert_eq!(edges[0].resolution_outcome, "resolved_internal");
        assert!((edges[0].confidence_weight - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn resolve_imports_uses_to_name_for_unresolved_target() {
        let conn = setup_test_db();
        let imports = vec![RawImport {
            source_qualified_name: source_symbol_id_for_path("src/lib.rs"),
            target_qualified_name: "missing::Symbol".to_string(),
            target_name: "Symbol".to_string(),
            import_line: 8,
        }];

        let edges = resolve_imports(&conn, imports, "my-repo", "main").unwrap();
        assert_eq!(edges.len(), 1);
        assert!(edges[0].to_symbol_id.is_none());
        assert_eq!(edges[0].to_name.as_deref(), Some("Symbol"));
        assert_eq!(edges[0].confidence, "medium");
        assert_eq!(edges[0].resolution_outcome, "external_reference");
        assert!((edges[0].confidence_weight - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn resolve_imports_assigns_low_confidence_for_unresolved_internal_name() {
        let conn = setup_test_db();
        let imports = vec![RawImport {
            source_qualified_name: "module::caller".to_string(),
            target_qualified_name: "Symbol".to_string(),
            target_name: "Symbol".to_string(),
            import_line: 12,
        }];

        let edges = resolve_imports(&conn, imports, "my-repo", "main").unwrap();
        assert_eq!(edges.len(), 1);
        assert!(edges[0].to_symbol_id.is_none());
        assert_eq!(edges[0].confidence, "low");
        assert_eq!(edges[0].resolution_outcome, "unresolved");
        assert!((edges[0].confidence_weight - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn extract_imports_dispatch_rust_nested_use() {
        let source = r#"
use crate::auth::Claims;
use crate::auth::{validate_token, refresh_token};
use crate::db::*;
use crate::types::User as AppUser;
"#;
        let tree = parser::parse_file(source, "rust").unwrap();
        let imports = extract_imports(&tree, source, "rust", "src/lib.rs");
        let targets: HashSet<String> = imports
            .into_iter()
            .map(|i| i.target_qualified_name)
            .collect();
        assert!(targets.contains("auth::Claims"));
        assert!(targets.contains("auth::validate_token"));
        assert!(targets.contains("auth::refresh_token"));
        assert!(targets.contains("db::*"));
        assert!(targets.contains("types::User"));
    }

    #[test]
    fn extract_imports_dispatch_typescript_variants() {
        let source = r#"
import { Router } from "./router";
import AuthClient from "./auth/client";
import * as Utils from "./utils";
const cfg = require("./config");
"#;
        let tree = parser::parse_file(source, "typescript").unwrap();
        let imports = extract_imports(&tree, source, "typescript", "src/index.ts");
        let names: HashSet<String> = imports.into_iter().map(|i| i.target_name).collect();
        assert!(names.contains("Router"));
        assert!(names.contains("AuthClient"));
        assert!(names.contains("Utils"));
        assert!(names.contains("cfg"));
    }

    #[test]
    fn extract_imports_dispatch_python_variants() {
        let source = r#"
import os
from auth.jwt import validate_token, Claims
from .models import User as AppUser
"#;
        let tree = parser::parse_file(source, "python").unwrap();
        let imports = extract_imports(&tree, source, "python", "pkg/handlers.py");
        let targets: HashSet<String> = imports
            .into_iter()
            .map(|i| i.target_qualified_name)
            .collect();
        assert!(targets.contains("os"));
        assert!(targets.contains("auth.jwt.validate_token"));
        assert!(targets.contains("auth.jwt.Claims"));
        assert!(targets.contains("pkg.models.User"));
    }

    #[test]
    fn extract_imports_dispatch_go_variants() {
        let source = r#"
import "fmt"
import (
    "github.com/org/pkg/auth"
    cfg "github.com/org/pkg/config"
)
"#;
        let tree = parser::parse_file(source, "go").unwrap();
        let imports = extract_imports(&tree, source, "go", "main.go");
        let targets: HashSet<String> = imports
            .into_iter()
            .map(|i| i.target_qualified_name)
            .collect();
        assert!(targets.contains("fmt"));
        assert!(targets.contains("github.com/org/pkg/auth"));
        assert!(targets.contains("github.com/org/pkg/config"));
    }
}
