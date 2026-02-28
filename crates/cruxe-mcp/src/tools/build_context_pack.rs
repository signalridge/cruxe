use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "build_context_pack".into(),
        description:
            "Build a deterministic, token-budgeted context pack with provenance and diagnostics."
                .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "query": {
                    "type": "string",
                    "description": "Retrieval query used to assemble the context pack."
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope."
                },
                "language": {
                    "type": "string",
                    "description": "Optional language filter."
                },
                "budget_tokens": {
                    "type": "integer",
                    "description": "Token budget for the pack payload (default: 4000, max: 200000).",
                    "default": 4000,
                    "minimum": 1,
                    "maximum": 200000
                },
                "max_candidates": {
                    "type": "integer",
                    "description": "Upper bound for retrieval candidates before budgeting (default: 72).",
                    "default": 72,
                    "minimum": 1
                },
                "mode": {
                    "type": "string",
                    "description": "Pack shaping mode: `full` or `edit_minimal` (`aider_minimal` is an alias of `edit_minimal`).",
                    "enum": ["full", "edit_minimal", "aider_minimal"],
                    "default": "full"
                },
                "section_caps": {
                    "type": "object",
                    "description": "Optional per-section target caps before overflow fallback.",
                    "properties": {
                        "definitions": {"type": "integer", "minimum": 0},
                        "usages": {"type": "integer", "minimum": 0},
                        "deps": {"type": "integer", "minimum": 0},
                        "tests": {"type": "integer", "minimum": 0},
                        "config": {"type": "integer", "minimum": 0},
                        "docs": {"type": "integer", "minimum": 0}
                    }
                }
            },
            "required": ["query"]
        }),
    }
}
