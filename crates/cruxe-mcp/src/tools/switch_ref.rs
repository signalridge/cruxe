use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "switch_ref".into(),
        description: "Switch default ref for subsequent tool calls in this workspace session."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "ref": {
                    "type": "string",
                    "description": "Target ref name."
                }
            },
            "required": ["ref"]
        }),
    }
}
