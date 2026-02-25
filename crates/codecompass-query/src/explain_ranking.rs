use crate::search;
use codecompass_core::error::StateError;
use codecompass_core::types::SourceLayer;
use codecompass_state::tantivy_index::IndexSet;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ExplainRankingError {
    #[error("result not found")]
    ResultNotFound,
    #[error(transparent)]
    State(#[from] StateError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RankingResultSummary {
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_layer: Option<SourceLayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RankingScoringBreakdown {
    pub bm25: f64,
    pub exact_match: f64,
    pub qualified_name: f64,
    pub path_affinity: f64,
    pub definition_boost: f64,
    pub kind_match: f64,
    pub total: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RankingScoringDetails {
    pub bm25_source: String,
    pub exact_match_reason: String,
    pub qualified_name_reason: String,
    pub path_affinity_reason: String,
    pub definition_boost_reason: String,
    pub kind_match_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RankingExplanation {
    pub query: String,
    pub result: RankingResultSummary,
    pub scoring: RankingScoringBreakdown,
    pub scoring_details: RankingScoringDetails,
}

#[allow(clippy::too_many_arguments)]
pub fn explain_ranking(
    index_set: &IndexSet,
    conn: Option<&Connection>,
    query: &str,
    result_path: &str,
    result_line_start: u32,
    ref_name: Option<&str>,
    language: Option<&str>,
    limit: usize,
) -> Result<RankingExplanation, ExplainRankingError> {
    let search_limit = limit.max(20);
    let response = search::search_code(
        index_set,
        conn,
        query,
        ref_name,
        language,
        search_limit,
        true,
    )
    .map_err(ExplainRankingError::State)?;
    let reasons = response.ranking_reasons.unwrap_or_default();

    let Some((index, result)) = response.results.iter().enumerate().find(|(_, candidate)| {
        candidate.path == result_path && candidate.line_start == result_line_start
    }) else {
        return Err(ExplainRankingError::ResultNotFound);
    };
    let reason =
        reasons
            .get(index)
            .ok_or(ExplainRankingError::State(StateError::result_not_found(
                result_path,
                result_line_start,
            )))?;

    Ok(RankingExplanation {
        query: query.to_string(),
        result: RankingResultSummary {
            path: result.path.clone(),
            line_start: result.line_start,
            line_end: result.line_end,
            kind: result.kind.clone(),
            name: result.name.clone(),
            source_layer: result.source_layer,
        },
        scoring: RankingScoringBreakdown {
            bm25: reason.bm25_score,
            exact_match: reason.exact_match_boost,
            qualified_name: reason.qualified_name_boost,
            path_affinity: reason.path_affinity,
            definition_boost: reason.definition_boost,
            kind_match: reason.kind_match,
            total: reason.final_score,
        },
        scoring_details: RankingScoringDetails {
            bm25_source: format!("tantivy.bm25 (score={:.3})", reason.bm25_score),
            exact_match_reason: component_reason(
                reason.exact_match_boost,
                "exact symbol match boost applied",
                "no exact symbol match boost",
            ),
            qualified_name_reason: component_reason(
                reason.qualified_name_boost,
                "qualified name match boost applied",
                "no qualified name match boost",
            ),
            path_affinity_reason: component_reason(
                reason.path_affinity,
                "path affinity boost applied",
                "no path affinity boost",
            ),
            definition_boost_reason: component_reason(
                reason.definition_boost,
                "definition preference boost applied",
                "no definition preference boost",
            ),
            kind_match_reason: component_reason(
                reason.kind_match,
                "kind-specific boost applied",
                "no kind-specific boost",
            ),
        },
    })
}

fn component_reason(value: f64, positive: &str, none: &str) -> String {
    if value > 0.0 {
        return format!("{positive} (contribution={value:.3})");
    }
    none.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codecompass_core::time::now_iso8601;
    use codecompass_core::types::{FileRecord, SymbolKind, SymbolRecord};
    use codecompass_indexer::writer;
    use codecompass_state::{db, schema};

    fn write_symbol_fixture(
        index_set: &IndexSet,
        conn: &Connection,
        repo: &str,
        ref_name: &str,
    ) -> Result<(), StateError> {
        let symbol = SymbolRecord {
            repo: repo.to_string(),
            r#ref: ref_name.to_string(),
            commit: None,
            path: "src/lib.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: "sym-1".to_string(),
            symbol_stable_id: "stable-1".to_string(),
            name: "validate_token".to_string(),
            qualified_name: "auth::validate_token".to_string(),
            kind: SymbolKind::Function,
            signature: Some("pub fn validate_token(token: &str)".to_string()),
            line_start: 3,
            line_end: 8,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: Some(
                "pub fn validate_token(token: &str) -> bool { !token.is_empty() }".to_string(),
            ),
        };
        let snippet = codecompass_core::types::SnippetRecord {
            repo: repo.to_string(),
            r#ref: ref_name.to_string(),
            commit: None,
            path: "src/lib.rs".to_string(),
            language: "rust".to_string(),
            chunk_type: "symbol_body".to_string(),
            imports: None,
            line_start: 3,
            line_end: 8,
            content: "pub fn validate_token(token: &str) -> bool { !token.is_empty() }".to_string(),
        };
        let file = FileRecord {
            repo: repo.to_string(),
            r#ref: ref_name.to_string(),
            commit: None,
            path: "src/lib.rs".to_string(),
            filename: "lib.rs".to_string(),
            language: "rust".to_string(),
            content_hash: blake3::hash(b"fixture").to_hex().to_string(),
            size_bytes: 120,
            updated_at: now_iso8601(),
            content_head: Some("pub fn validate_token(token: &str) -> bool".to_string()),
        };
        writer::write_file_records(index_set, conn, &[symbol], &[snippet], &file)
    }

    #[test]
    fn explain_ranking_is_deterministic_for_same_state() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = IndexSet::open(tmp.path()).unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        write_symbol_fixture(&index_set, &conn, "proj", "live").unwrap();

        let first = explain_ranking(
            &index_set,
            Some(&conn),
            "validate_token",
            "src/lib.rs",
            3,
            Some("live"),
            Some("rust"),
            20,
        )
        .unwrap();
        let second = explain_ranking(
            &index_set,
            Some(&conn),
            "validate_token",
            "src/lib.rs",
            3,
            Some("live"),
            Some("rust"),
            20,
        )
        .unwrap();

        assert_eq!(first, second);
        assert!(first.scoring.total > 0.0);
    }

    #[test]
    fn explain_ranking_scoring_components_sum_to_total() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = IndexSet::open(tmp.path()).unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        write_symbol_fixture(&index_set, &conn, "proj", "live").unwrap();

        let explanation = explain_ranking(
            &index_set,
            Some(&conn),
            "validate_token",
            "src/lib.rs",
            3,
            Some("live"),
            Some("rust"),
            20,
        )
        .unwrap();

        let scoring = &explanation.scoring;
        let sum = scoring.bm25
            + scoring.exact_match
            + scoring.qualified_name
            + scoring.path_affinity
            + scoring.definition_boost
            + scoring.kind_match;
        assert!(
            (sum - scoring.total).abs() < 1e-6,
            "sum={} total={}",
            sum,
            scoring.total
        );
    }
}
