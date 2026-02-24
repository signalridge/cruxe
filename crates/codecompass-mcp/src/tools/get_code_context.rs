use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "get_code_context".into(),
        description:
            "Retrieve code context fitted to a token budget using breadth/depth strategies.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query for relevant code context"
                },
                "max_tokens": {
                    "type": "integer",
                    "description": "Maximum estimated tokens in returned context (default: 4000)",
                    "default": 4000
                },
                "strategy": {
                    "type": "string",
                    "description": "\"breadth\" (default) or \"depth\"",
                    "enum": ["breadth", "depth"],
                    "default": "breadth"
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope"
                },
                "language": {
                    "type": "string",
                    "description": "Language filter"
                }
            },
            "required": ["query"]
        }),
    }
}
