use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "sync_repo".into(),
        description: "Trigger incremental sync based on file changes since last indexed state."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "force": {
                    "type": "boolean",
                    "description": "Force full sync"
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope"
                }
            }
        }),
    }
}
