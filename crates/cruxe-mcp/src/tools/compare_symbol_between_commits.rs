use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "compare_symbol_between_commits".into(),
        description: "Compare one symbol across two refs and summarize signature/body/line deltas."
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
                    "description": "Symbol name (or qualified name) to compare."
                },
                "path": {
                    "type": "string",
                    "description": "Optional file path to disambiguate symbols."
                },
                "base_ref": {
                    "type": "string",
                    "description": "Base ref (commit, branch, or tag)."
                },
                "head_ref": {
                    "type": "string",
                    "description": "Head ref (commit, branch, or tag)."
                }
            },
            "required": ["symbol_name", "base_ref", "head_ref"]
        }),
    }
}
