use cruxe_core::config::SearchConfig;
use cruxe_core::types::{QueryIntent, SymbolKind, SymbolRecord};
use cruxe_query::search::{SearchExecutionOptions, search_code_with_options};
use cruxe_state::embedding;
use cruxe_state::tantivy_index::IndexSet;
use cruxe_state::vector_index::{self, VectorRecord};
use tempfile::tempdir;

fn hybrid_search_config() -> SearchConfig {
    let mut config = SearchConfig::default();
    config.semantic.mode = "hybrid".to_string();
    config.semantic.ratio = 0.7;
    config.semantic.lexical_short_circuit_threshold = 0.99;
    config.semantic.embedding.provider = "local".to_string();
    config.semantic.embedding.profile = "fast_local".to_string();
    config.semantic.embedding.model = "NomicEmbedTextV15Q".to_string();
    config.semantic.embedding.model_version = "fastembed-1".to_string();
    config.semantic.embedding.dimensions = 768;
    config
}

#[test]
fn hybrid_mode_returns_semantic_match_for_conceptual_query_without_keyword_overlap() {
    let workspace = tempdir().unwrap();
    let db_path = workspace.path().join("state.db");
    let conn = cruxe_state::db::open_connection(&db_path).unwrap();
    cruxe_state::schema::create_tables(&conn).unwrap();

    let index_root = workspace.path().join("index");
    let index_set = IndexSet::open_at(&index_root).unwrap();

    let config = hybrid_search_config();
    let conceptual_query = "how can i keep users signed in longer after their login expires";
    let mut provider = embedding::build_embedding_provider(&config.semantic)
        .unwrap()
        .provider;
    let query_vector = provider
        .embed_batch(&[conceptual_query.to_string()])
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let anti_vector: Vec<f32> = query_vector.iter().map(|value| -*value).collect();

    vector_index::upsert_vectors(
        &conn,
        &[
            VectorRecord {
                project_id: "proj".to_string(),
                ref_name: "main".to_string(),
                symbol_stable_id: "stable-semantic-hit".to_string(),
                snippet_hash: "hash-semantic-hit".to_string(),
                embedding_model_id: "NomicEmbedTextV15Q".to_string(),
                embedding_model_version: "fastembed-1".to_string(),
                embedding_dimensions: query_vector.len(),
                path: "src/token_lifecycle.rs".to_string(),
                line_start: 10,
                line_end: 18,
                language: "rust".to_string(),
                chunk_type: Some("function_body".to_string()),
                snippet_text: "fn ttl_refresh() { grant_access_window(); }".to_string(),
                vector: query_vector.clone(),
            },
            VectorRecord {
                project_id: "proj".to_string(),
                ref_name: "main".to_string(),
                symbol_stable_id: "stable-low-rank".to_string(),
                snippet_hash: "hash-low-rank".to_string(),
                embedding_model_id: "NomicEmbedTextV15Q".to_string(),
                embedding_model_version: "fastembed-1".to_string(),
                embedding_dimensions: anti_vector.len(),
                path: "src/cache_eviction.rs".to_string(),
                line_start: 4,
                line_end: 12,
                language: "rust".to_string(),
                chunk_type: Some("function_body".to_string()),
                snippet_text: "fn purge_entries() { evict_all(); }".to_string(),
                vector: anti_vector,
            },
        ],
    )
    .unwrap();
    cruxe_state::symbols::insert_symbol(
        &conn,
        &SymbolRecord {
            repo: "proj".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/token_lifecycle.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: "sym-semantic-hit".to_string(),
            symbol_stable_id: "stable-semantic-hit".to_string(),
            name: "ttl_refresh".to_string(),
            qualified_name: "ttl_refresh".to_string(),
            kind: SymbolKind::Function,
            signature: Some("fn ttl_refresh()".to_string()),
            line_start: 10,
            line_end: 18,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: Some("fn ttl_refresh() { grant_access_window(); }".to_string()),
        },
    )
    .unwrap();
    cruxe_state::symbols::insert_symbol(
        &conn,
        &SymbolRecord {
            repo: "proj".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/cache_eviction.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: "sym-low-rank".to_string(),
            symbol_stable_id: "stable-low-rank".to_string(),
            name: "purge_entries".to_string(),
            qualified_name: "purge_entries".to_string(),
            kind: SymbolKind::Function,
            signature: Some("fn purge_entries()".to_string()),
            line_start: 4,
            line_end: 12,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: Some("fn purge_entries() { evict_all(); }".to_string()),
        },
    )
    .unwrap();

    let response = search_code_with_options(
        &index_set,
        Some(&conn),
        conceptual_query,
        Some("main"),
        None,
        5,
        false,
        SearchExecutionOptions {
            search_config: config,
            semantic_ratio_override: None,
            confidence_threshold_override: None,
            role: None,
            plan_override: None,
            policy_mode_override: None,
            policy_runtime: None,
        },
    )
    .unwrap();

    assert_eq!(response.query_intent, QueryIntent::NaturalLanguage);
    assert!(response.metadata.semantic_triggered);
    assert_eq!(response.metadata.semantic_skipped_reason, None);
    assert!(!response.results.is_empty());
    assert_eq!(
        response.results[0].symbol_stable_id.as_deref(),
        Some("stable-semantic-hit")
    );
    assert_eq!(response.results[0].provenance, "semantic");
    assert!(
        !conceptual_query.contains("ttl_refresh")
            && !conceptual_query.contains("grant_access_window")
            && !conceptual_query.contains("purge_entries"),
        "query should not contain direct snippet keywords"
    );
}
