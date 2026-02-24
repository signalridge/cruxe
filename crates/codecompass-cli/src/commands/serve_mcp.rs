use anyhow::{Context, Result};
use codecompass_core::types::WorkspaceConfig;
use std::path::Path;

pub fn run(
    workspace: &Path,
    config_file: Option<&Path>,
    no_prewarm: bool,
    workspace_config: WorkspaceConfig,
) -> Result<()> {
    let workspace = std::fs::canonicalize(workspace).context("Failed to resolve workspace path")?;

    codecompass_mcp::server::run_server(&workspace, config_file, no_prewarm, workspace_config)
        .map_err(|e| anyhow::anyhow!("MCP server error: {}", e))
}

/// Start the MCP server in HTTP transport mode (T227).
pub fn run_http(
    workspace: &Path,
    config_file: Option<&Path>,
    no_prewarm: bool,
    workspace_config: WorkspaceConfig,
    bind_addr: &str,
    port: u16,
) -> Result<()> {
    let workspace = std::fs::canonicalize(workspace).context("Failed to resolve workspace path")?;

    let rt = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;
    rt.block_on(codecompass_mcp::http::run_http_server(
        &workspace,
        config_file,
        no_prewarm,
        workspace_config,
        bind_addr,
        port,
    ))
    .map_err(|e| anyhow::anyhow!("MCP HTTP server error: {}", e))
}
