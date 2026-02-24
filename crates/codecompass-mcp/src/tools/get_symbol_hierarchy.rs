use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "get_symbol_hierarchy".into(),
        description:
            "Traverse the parent chain (ancestors) or child tree (descendants) for a symbol.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "symbol_name": {
                    "type": "string",
                    "description": "Symbol name to start from"
                },
                "path": {
                    "type": "string",
                    "description": "File path to disambiguate symbols with the same name; omitted may return ambiguous_symbol if multiple files match"
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope"
                },
                "direction": {
                    "type": "string",
                    "description": "\"ancestors\" (default) or \"descendants\"",
                    "enum": ["ancestors", "descendants"],
                    "default": "ancestors"
                }
            },
            "required": ["symbol_name"]
        }),
    }
}
