use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "explain_ranking".into(),
        description: "Explain deterministic ranking contributions for one search result.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "query": {
                    "type": "string",
                    "description": "Search query string."
                },
                "result_path": {
                    "type": "string",
                    "description": "Result file path to explain."
                },
                "result_line_start": {
                    "type": "integer",
                    "description": "Result start line to explain."
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope."
                },
                "language": {
                    "type": "string",
                    "description": "Optional language filter for re-executed query."
                },
                "limit": {
                    "type": "integer",
                    "description": "Search candidate limit for explain lookup (default: 200)."
                }
            },
            "required": ["query", "result_path", "result_line_start"]
        }),
    }
}
