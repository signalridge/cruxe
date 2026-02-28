use anyhow::{Context, Result};
use cruxe_core::config::Config;
use cruxe_core::constants;
use cruxe_core::time::now_iso8601;
use cruxe_core::types::{Project, generate_project_id};
use cruxe_core::vcs;
use cruxe_state::{db, project, schema, tantivy_index};
use std::path::Path;
use tracing::info;

pub fn run(repo_root: &Path, config_file: Option<&Path>) -> Result<()> {
    let repo_root = std::fs::canonicalize(repo_root).context("Failed to resolve project path")?;
    let repo_root_str = repo_root.to_string_lossy().to_string();

    let config = Config::load_with_file(Some(&repo_root), config_file)?;
    let project_id = generate_project_id(&repo_root_str);
    let data_dir = config.project_data_dir(&project_id);

    // Create data directory
    std::fs::create_dir_all(&data_dir).context("Failed to create data directory")?;

    // Open SQLite and create schema
    let db_path = data_dir.join(constants::STATE_DB_FILE);
    let conn = db::open_connection(&db_path)?;
    schema::create_tables(&conn)?;

    // Check for existing project
    if let Some(existing) = project::get_by_root(&conn, &repo_root_str)? {
        println!("Project already initialized:");
        println!("  ID:       {}", existing.project_id);
        println!("  Root:     {}", existing.repo_root);
        println!(
            "  VCS mode: {}",
            if existing.vcs_mode { "yes" } else { "no" }
        );
        println!("  Data dir: {}", data_dir.display());
        return Ok(());
    }

    // Detect VCS mode
    let vcs_mode = vcs::is_git_repo(&repo_root);
    let default_ref = if vcs_mode {
        vcs::detect_default_ref(&repo_root, "main")
    } else {
        constants::REF_LIVE.to_string()
    };

    let now = now_iso8601();
    let project = Project {
        project_id: project_id.clone(),
        repo_root: repo_root_str.clone(),
        display_name: repo_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string()),
        default_ref,
        vcs_mode,
        schema_version: constants::SCHEMA_VERSION,
        parser_version: constants::PARSER_VERSION,
        created_at: now.clone(),
        updated_at: now,
    };

    project::create_project(&conn, &project)?;

    // Create Tantivy index directories
    let _index_set = tantivy_index::IndexSet::open(&data_dir)?;

    println!("Project initialized successfully!");
    println!("  ID:       {}", project_id);
    println!("  Root:     {}", repo_root_str);
    println!("  VCS mode: {}", if vcs_mode { "yes" } else { "no" });
    println!("  Data dir: {}", data_dir.display());
    println!();
    println!("Next step: run `cruxe index` to index your codebase.");

    info!(project_id, repo_root = %repo_root_str, vcs_mode, "Project initialized");
    Ok(())
}
