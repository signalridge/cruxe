use super::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "locate_symbol".into(),
        description: "Find symbol definitions by name. Returns precise file:line locations.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Symbol name to locate"
                },
                "kind": {
                    "type": "string",
                    "description": "Filter by kind (fn, struct, class, method, etc.)"
                },
                "language": {
                    "type": "string",
                    "description": "Filter by language"
                },
                "ref": {
                    "type": "string",
                    "description": "Branch/ref scope"
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
                    "description": "Token-thrifty serialization flag. Works with all detail levels."
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
                }
            },
            "required": ["name"]
        }),
    }
}
