use anyhow::{Context, Result, bail};
use codecompass_core::config::Config;
use codecompass_core::constants;
use codecompass_core::types::generate_project_id;
use codecompass_state::{db, export, project, schema};
use std::path::Path;

pub fn run(workspace: &Path, output_path: &Path, config_file: Option<&Path>) -> Result<()> {
    let workspace = std::fs::canonicalize(workspace).context("Failed to resolve workspace path")?;
    let workspace_str = workspace.to_string_lossy().to_string();
    let config = Config::load_with_file(Some(&workspace), config_file)?;
    let project_id = generate_project_id(&workspace_str);
    let data_dir = config.project_data_dir(&project_id);
    let db_path = data_dir.join(constants::STATE_DB_FILE);
    let conn = db::open_connection(&db_path)?;
    schema::create_tables(&conn)?;

    let Some(project_row) = project::get_by_root(&conn, &workspace_str)? else {
        bail!(
            "Project not initialized. Run `codecompass init --path {}` first.",
            workspace_str
        );
    };

    let metadata = export::PortableStateMetadata::new(
        project_row.schema_version,
        project_row.parser_version,
        project_row.project_id,
        workspace_str.clone(),
    );
    export::export_bundle(&data_dir, output_path, &metadata)?;

    println!("State export complete");
    println!("  Workspace: {}", workspace_str);
    println!("  Project ID: {}", metadata.project_id);
    println!("  Output: {}", output_path.display());
    Ok(())
}
