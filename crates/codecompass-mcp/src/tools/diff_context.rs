use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "diff_context".into(),
        description: "Summarize symbol-level changes between two refs.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "base_ref": {
                    "type": "string",
                    "description": "Base ref. Default: project default branch."
                },
                "head_ref": {
                    "type": "string",
                    "description": "Head ref. Default: current effective ref."
                },
                "path_filter": {
                    "type": "string",
                    "description": "Optional path prefix filter."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max symbol changes to return (default: 50)."
                }
            }
        }),
    }
}
