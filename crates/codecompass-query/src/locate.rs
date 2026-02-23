use codecompass_core::error::StateError;
use serde::{Deserialize, Serialize};
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, TermQuery};
use tantivy::schema::IndexRecordOption;
use tantivy::schema::Value;
use tantivy::{Index, Term};
use tracing::debug;

/// A located symbol result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocateResult {
    pub symbol_id: String,
    pub symbol_stable_id: String,
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub kind: String,
    pub name: String,
    pub qualified_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    pub score: f32,
}

/// Locate symbols by name in the Tantivy symbols index.
pub fn locate_symbol(
    index: &Index,
    name: &str,
    kind: Option<&str>,
    language: Option<&str>,
    r#ref: Option<&str>,
    limit: usize,
) -> Result<Vec<LocateResult>, StateError> {
    let reader = index.reader().map_err(StateError::tantivy)?;
    let searcher = reader.searcher();
    let schema = index.schema();

    let symbol_exact = schema
        .get_field("symbol_exact")
        .map_err(StateError::tantivy)?;

    // Build boolean query
    let mut clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::new();

    // Must match symbol name
    clauses.push((
        Occur::Must,
        Box::new(TermQuery::new(
            Term::from_field_text(symbol_exact, name),
            IndexRecordOption::Basic,
        )),
    ));

    // Optional kind filter
    if let Some(k) = kind {
        let kind_field = schema.get_field("kind").map_err(StateError::tantivy)?;
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(kind_field, k),
                IndexRecordOption::Basic,
            )),
        ));
    }

    // Optional language filter
    if let Some(lang) = language {
        let lang_field = schema.get_field("language").map_err(StateError::tantivy)?;
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(lang_field, lang),
                IndexRecordOption::Basic,
            )),
        ));
    }

    // Optional ref filter
    if let Some(r) = r#ref {
        let ref_field = schema.get_field("ref").map_err(StateError::tantivy)?;
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(ref_field, r),
                IndexRecordOption::Basic,
            )),
        ));
    }

    let query = BooleanQuery::new(clauses);

    let top_docs = searcher
        .search(&query, &TopDocs::with_limit(limit))
        .map_err(StateError::tantivy)?;

    let mut results = Vec::new();
    for (score, doc_address) in top_docs {
        let doc = searcher
            .doc::<tantivy::TantivyDocument>(doc_address)
            .map_err(StateError::tantivy)?;

        let get_text = |field_name: &str| -> String {
            schema
                .get_field(field_name)
                .ok()
                .and_then(|f| doc.get_first(f))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let get_u64 = |field_name: &str| -> u64 {
            schema
                .get_field(field_name)
                .ok()
                .and_then(|f| doc.get_first(f))
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        };

        let opt_text = |field_name: &str| -> Option<String> {
            let s = get_text(field_name);
            if s.is_empty() { None } else { Some(s) }
        };

        results.push(LocateResult {
            symbol_id: get_text("symbol_id"),
            symbol_stable_id: get_text("symbol_stable_id"),
            path: get_text("path"),
            line_start: get_u64("line_start") as u32,
            line_end: get_u64("line_end") as u32,
            kind: get_text("kind"),
            name: get_text("symbol_exact"),
            qualified_name: get_text("qualified_name"),
            signature: opt_text("signature"),
            language: get_text("language"),
            visibility: opt_text("visibility"),
            score,
        });
    }

    debug!(name, results = results.len(), "locate_symbol");
    Ok(results)
}
