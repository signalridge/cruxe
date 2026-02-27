use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "index_repo".into(),
        description: "Trigger full or incremental indexing of a registered project.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "force": {
                    "type": "boolean",
                    "description": "Force full re-index"
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope"
                }
            }
        }),
    }
}
