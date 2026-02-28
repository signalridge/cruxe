use anyhow::{Context, Result};
use cruxe_core::config::Config;
use cruxe_core::constants;
use cruxe_core::types::generate_project_id;
use cruxe_core::vcs;
use cruxe_query::search;
use cruxe_state::{db, project, schema, tantivy_index::IndexSet};
use std::path::Path;

pub fn run(
    repo_root: &Path,
    query: &str,
    r#ref: Option<&str>,
    language: Option<&str>,
    limit: usize,
    config_file: Option<&Path>,
) -> Result<()> {
    let repo_root = std::fs::canonicalize(repo_root).context("Failed to resolve project path")?;
    let repo_root_str = repo_root.to_string_lossy().to_string();

    let config = Config::load_with_file(Some(&repo_root), config_file)?;
    let project_id = generate_project_id(&repo_root_str);
    let data_dir = config.project_data_dir(&project_id);
    let db_path = data_dir.join(constants::STATE_DB_FILE);

    let index_set = IndexSet::open_existing(&data_dir).map_err(|e| match e {
        cruxe_core::error::StateError::SchemaMigrationRequired { .. }
        | cruxe_core::error::StateError::CorruptManifest(_) => {
            anyhow::anyhow!("Index schema is incompatible. Run `cruxe index --force`.")
        }
        _ => anyhow::anyhow!("Failed to open indices: {}. Run `cruxe index` first.", e),
    })?;

    let conn = db::open_connection_with_config(
        &db_path,
        config.storage.busy_timeout_ms,
        config.storage.cache_size,
    )
    .map_err(|e| anyhow::anyhow!("Failed to open state DB: {}", e))?;
    schema::create_tables(&conn)
        .map_err(|e| anyhow::anyhow!("Failed to initialize schema: {}", e))?;
    let proj = project::get_by_root(&conn, &repo_root_str)?
        .ok_or_else(|| anyhow::anyhow!("Project not initialized. Run `cruxe init` first."))?;
    let resolved_ref = vcs::resolve_effective_ref(&repo_root, r#ref, &proj.default_ref);
    let response = search::search_code(
        &index_set,
        Some(&conn),
        query,
        Some(&resolved_ref),
        language,
        limit,
        false,
    )
    .map_err(|e| anyhow::anyhow!("Search failed: {}", e))?;

    println!("Query intent: {:?}", response.query_intent);
    println!(
        "Results: {} (of {} candidates)",
        response.results.len(),
        response.total_candidates
    );
    println!();

    if response.results.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    // Print results as table
    println!(
        "{:<50} {:<10} {:<20} {:<8}",
        "PATH", "KIND", "NAME", "SCORE"
    );
    println!("{}", "-".repeat(88));

    for result in &response.results {
        let location = if result.line_start > 0 {
            format!("{}:{}", result.path, result.line_start)
        } else {
            result.path.clone()
        };
        println!(
            "{:<50} {:<10} {:<20} {:<8.2}",
            location,
            result.kind.as_deref().unwrap_or("-"),
            result.name.as_deref().unwrap_or("-"),
            result.score,
        );
    }

    Ok(())
}
