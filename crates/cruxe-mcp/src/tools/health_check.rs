use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "health_check".into(),
        description: "Return project-level operational status. Checks Tantivy indices, SQLite integrity, grammar availability, and prewarm status.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                }
            }
        }),
    }
}
