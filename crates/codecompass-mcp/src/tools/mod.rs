pub mod get_file_outline;
pub mod health_check;
pub mod index_repo;
pub mod index_status;
pub mod locate_symbol;
pub mod search_code;
pub mod sync_repo;

use serde::{Deserialize, Serialize};

/// MCP tool definition for tools/list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// Return all tool definitions.
pub fn list_tools() -> Vec<ToolDefinition> {
    vec![
        index_repo::definition(),
        sync_repo::definition(),
        search_code::definition(),
        locate_symbol::definition(),
        get_file_outline::definition(),
        health_check::definition(),
        index_status::definition(),
    ]
}
