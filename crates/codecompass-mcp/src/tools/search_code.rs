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
                }
            },
            "required": ["query"]
        }),
    }
}
