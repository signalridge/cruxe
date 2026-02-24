use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    #[error("state error: {0}")]
    State(#[from] StateError),

    #[error("index error: {0}")]
    Index(#[from] IndexError),

    #[error("parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("query error: {0}")]
    Query(#[from] QueryError),

    #[error("mcp error: {0}")]
    Mcp(#[from] McpError),

    #[error("vcs error: {0}")]
    Vcs(#[from] VcsError),

    #[error("workspace error: {0}")]
    Workspace(#[from] WorkspaceError),
}

#[derive(Error, Debug)]
pub enum WorkspaceError {
    #[error("workspace not registered: {path}")]
    NotRegistered { path: String },

    #[error("workspace not allowed: {path} ({reason})")]
    NotAllowed { path: String, reason: String },

    #[error("auto-workspace is disabled; enable with --auto-workspace")]
    AutoDiscoveryDisabled,

    #[error("workspace limit exceeded: max {max} auto-discovered workspaces")]
    LimitExceeded { max: usize },

    #[error("--allowed-root is required when --auto-workspace is enabled")]
    AllowedRootRequired,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("config file not found: {path}")]
    NotFound { path: String },

    #[error("failed to parse config: {0}")]
    ParseError(String),

    #[error("invalid config value: {field}: {reason}")]
    InvalidValue { field: String, reason: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum StateError {
    #[error("sqlite error: {0}")]
    Sqlite(String),

    #[error("tantivy error: {0}")]
    Tantivy(String),

    #[error("project not found: {project_id}")]
    ProjectNotFound { project_id: String },

    #[error("project already exists: {repo_root}")]
    ProjectAlreadyExists { repo_root: String },

    #[error("schema migration required: current={current}, required={required}")]
    SchemaMigrationRequired { current: u32, required: u32 },

    #[error("corrupt manifest: {0}")]
    CorruptManifest(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl StateError {
    /// Convenience constructor for SQLite errors — use with `.map_err(StateError::sqlite)`.
    pub fn sqlite<E: std::fmt::Display>(e: E) -> Self {
        Self::Sqlite(e.to_string())
    }

    /// Convenience constructor for Tantivy errors — use with `.map_err(StateError::tantivy)`.
    pub fn tantivy<E: std::fmt::Display>(e: E) -> Self {
        Self::Tantivy(e.to_string())
    }
}

#[derive(Error, Debug)]
pub enum IndexError {
    #[error("index in progress: job_id={job_id}")]
    InProgress { job_id: String },

    #[error("index incompatible: {reason}")]
    Incompatible { reason: String },

    #[error("file too large: {path} ({size} bytes)")]
    FileTooLarge { path: String, size: u64 },

    #[error("unsupported language: {language}")]
    UnsupportedLanguage { language: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("state error: {0}")]
    State(#[from] StateError),
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("tree-sitter parse failed: {path}")]
    TreeSitterFailed { path: String },

    #[error("grammar not available: {language}")]
    GrammarNotAvailable { language: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("index not ready: {0}")]
    IndexNotReady(String),

    #[error("invalid query: {0}")]
    InvalidQuery(String),

    #[error("state error: {0}")]
    State(#[from] StateError),
}

#[derive(Error, Debug)]
pub enum McpError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("method not found: {method}")]
    MethodNotFound { method: String },

    #[error("internal error: {0}")]
    Internal(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum VcsError {
    #[error("not a git repository: {path}")]
    NotGitRepo { path: String },

    #[error("git error: {0}")]
    GitError(String),
}

pub type Result<T> = std::result::Result<T, Error>;
