use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "suggest_followup_queries".into(),
        description: "Suggest next tool calls when prior results are low-confidence or empty."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "workspace": {
                    "type": "string",
                    "description": "Absolute path to target workspace. Default: server's default project."
                },
                "previous_query": {
                    "type": "object",
                    "description": "The previous tool call, including tool name and params.",
                    "properties": {
                        "tool": { "type": "string" },
                        "params": { "type": "object" }
                    },
                    "required": ["tool"]
                },
                "previous_results": {
                    "type": "object",
                    "description": "Result summary from the previous query (top_score, total_candidates, query_intent, etc.)."
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope."
                },
                "confidence_threshold": {
                    "type": "number",
                    "description": "Threshold below which results are considered low-confidence. Default: 0.5."
                }
            },
            "required": ["previous_query", "previous_results"]
        }),
    }
}
