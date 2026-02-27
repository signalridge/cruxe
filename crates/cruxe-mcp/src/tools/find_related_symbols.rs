use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "find_related_symbols".into(),
        description: "Find symbols in the same file/module/package scope as an anchor symbol."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "symbol_name": {
                    "type": "string",
                    "description": "Anchor symbol name"
                },
                "path": {
                    "type": "string",
                    "description": "File path to disambiguate symbols with same name; omitted may return ambiguous_symbol if multiple files match"
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope"
                },
                "scope": {
                    "type": "string",
                    "description": "\"file\" (default), \"module\", or \"package\"",
                    "enum": ["file", "module", "package"],
                    "default": "file"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max related symbols (default: 20)",
                    "default": 20
                }
            },
            "required": ["symbol_name"]
        }),
    }
}
