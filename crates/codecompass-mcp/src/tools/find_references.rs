use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "find_references".into(),
        description: "Find symbol references using relation graph edges.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "symbol_name": {
                    "type": "string",
                    "description": "Name or qualified name of the symbol."
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope."
                },
                "kind": {
                    "type": "string",
                    "description": "Optional edge type filter (imports, calls, implements, extends, references)."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max references to return (default: 20)."
                }
            },
            "required": ["symbol_name"]
        }),
    }
}
