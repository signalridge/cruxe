use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "get_file_outline".into(),
        description: "Return a nested symbol tree for a source file. Shows structure without reading full file content.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "path": {
                    "type": "string",
                    "description": "Source file path relative to repo root"
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope"
                },
                "depth": {
                    "type": "string",
                    "description": "\"top\" (top-level only) or \"all\" (nested). Default: \"all\"",
                    "enum": ["top", "all"]
                },
                "language": {
                    "type": "string",
                    "description": "Filter hint (informational)"
                }
            },
            "required": ["path"]
        }),
    }
}
