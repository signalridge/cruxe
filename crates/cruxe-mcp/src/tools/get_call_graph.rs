use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "get_call_graph".into(),
        description: "Return callers/callees for a symbol with bounded graph traversal.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "symbol_name": {
                    "type": "string",
                    "description": "Name (or qualified name) of the symbol to inspect."
                },
                "path": {
                    "type": "string",
                    "description": "Optional file path to disambiguate symbols."
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope."
                },
                "direction": {
                    "type": "string",
                    "enum": ["callers", "callees", "both"],
                    "description": "Traversal direction. Default: both."
                },
                "depth": {
                    "type": "integer",
                    "description": "Traversal depth (1-5). Values above 5 are clamped."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max edges returned per direction (default: 20)."
                }
            },
            "required": ["symbol_name"]
        }),
    }
}
