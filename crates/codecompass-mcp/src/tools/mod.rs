pub mod compare_symbol_between_commits;
pub mod diff_context;
pub mod explain_ranking;
pub mod find_references;
pub mod find_related_symbols;
pub mod get_call_graph;
pub mod get_code_context;
pub mod get_file_outline;
pub mod get_symbol_hierarchy;
pub mod health_check;
pub mod index_repo;
pub mod index_status;
pub mod list_refs;
pub mod locate_symbol;
pub mod search_code;
pub mod suggest_followup_queries;
pub mod switch_ref;
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
        get_call_graph::definition(),
        compare_symbol_between_commits::definition(),
        get_symbol_hierarchy::definition(),
        find_related_symbols::definition(),
        get_code_context::definition(),
        suggest_followup_queries::definition(),
        health_check::definition(),
        index_status::definition(),
        diff_context::definition(),
        find_references::definition(),
        explain_ranking::definition(),
        list_refs::definition(),
        switch_ref::definition(),
    ]
}
