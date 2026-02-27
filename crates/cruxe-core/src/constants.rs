/// Default ref for single-version (non-VCS) mode.
pub const REF_LIVE: &str = "live";

/// Default result limit for queries.
pub const DEFAULT_LIMIT: usize = 10;

/// Current schema version for SQLite tables.
pub const SCHEMA_VERSION: u32 = 1;

/// Current parser version for tree-sitter extraction.
pub const PARSER_VERSION: u32 = 1;

/// Maximum file size to index (1MB).
pub const MAX_FILE_SIZE: u64 = 1_048_576;

/// Default data directory name under home.
pub const DEFAULT_DATA_DIR: &str = ".cruxe";

/// Protocol version for MCP responses.
pub const PROTOCOL_VERSION: &str = "1.0";

/// Stable ID version prefix.
pub const STABLE_ID_VERSION: &str = "stable_id:v1";

/// Project config file name.
pub const PROJECT_CONFIG_FILE: &str = ".cruxe/config.toml";

/// Ignore file name.
pub const IGNORE_FILE: &str = ".cruxeignore";

/// SQLite database file name.
pub const STATE_DB_FILE: &str = "state.db";
