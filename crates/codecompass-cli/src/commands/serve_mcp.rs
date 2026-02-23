use anyhow::{Context, Result};
use std::path::Path;

pub fn run(
    workspace: &Path,
    config_file: Option<&Path>,
    no_prewarm: bool,
    enable_ranking_reasons: bool,
) -> Result<()> {
    let workspace = std::fs::canonicalize(workspace).context("Failed to resolve workspace path")?;

    codecompass_mcp::server::run_server(&workspace, config_file, no_prewarm, enable_ranking_reasons)
        .map_err(|e| anyhow::anyhow!("MCP server error: {}", e))
}
