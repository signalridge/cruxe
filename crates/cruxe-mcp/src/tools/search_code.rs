use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "search_code".into(),
        description: "Search across symbols, snippets, and files with query intent classification."
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
                    "description": "Search query (symbol name, path, error string, or natural language)"
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope"
                },
                "language": {
                    "type": "string",
                    "description": "Filter by language"
                },
                "role": {
                    "type": "string",
                    "description": "Filter by semantic symbol role",
                    "enum": ["type", "callable", "value", "namespace", "alias"]
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results (default: 10)"
                },
                "detail_level": {
                    "type": "string",
                    "description": "Response verbosity: \"location\", \"signature\" (default), \"context\"",
                    "enum": ["location", "signature", "context"]
                },
                "compact": {
                    "type": "boolean",
                    "description": "Token-thrifty serialization flag. Keeps identity/location/score fields while omitting large context blocks."
                },
                "freshness_policy": {
                    "type": "string",
                    "description": "Freshness behavior: \"strict\", \"balanced\" (default), \"best_effort\"",
                    "enum": ["strict", "balanced", "best_effort"]
                },
                "ranking_explain_level": {
                    "type": "string",
                    "description": "Ranking explainability payload level: \"off\" (default), \"basic\", \"full\"",
                    "enum": ["off", "basic", "full"]
                },
                "semantic_ratio": {
                    "type": "number",
                    "description": "Optional semantic blend ratio cap override (0.0-1.0). Runtime may reduce actual usage.",
                    "minimum": 0.0,
                    "maximum": 1.0
                },
                "confidence_threshold": {
                    "type": "number",
                    "description": "Optional low-confidence threshold override (0.0-1.0).",
                    "minimum": 0.0,
                    "maximum": 1.0
                },
                "plan": {
                    "type": "string",
                    "description": "Optional adaptive query plan override. Requires search.adaptive_plan.allow_override=true.",
                    "enum": ["lexical_fast", "hybrid_standard", "semantic_deep"]
                },
                "policy_mode": {
                    "type": "string",
                    "description": "Optional retrieval policy override when allowed by runtime policy config.",
                    "enum": ["strict", "balanced", "off", "audit_only"]
                }
            },
            "required": ["query"]
        }),
    }
}
