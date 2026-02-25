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

/// Canonical protocol-level error codes shared by MCP/HTTP transports.
///
/// Source of truth: `specs/meta/protocol-error-codes.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolErrorCode {
    InvalidInput,
    InvalidStrategy,
    InvalidMaxTokens,
    ProjectNotFound,
    WorkspaceNotRegistered,
    WorkspaceNotAllowed,
    WorkspaceLimitExceeded,
    IndexInProgress,
    IndexNotReady,
    SyncInProgress,
    IndexStale,
    IndexIncompatible,
    RefNotIndexed,
    OverlayNotReady,
    MergeBaseFailed,
    SymbolNotFound,
    AmbiguousSymbol,
    FileNotFound,
    ResultNotFound,
    NoEdgesAvailable,
    InternalError,
}

impl ProtocolErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::InvalidStrategy => "invalid_strategy",
            Self::InvalidMaxTokens => "invalid_max_tokens",
            Self::ProjectNotFound => "project_not_found",
            Self::WorkspaceNotRegistered => "workspace_not_registered",
            Self::WorkspaceNotAllowed => "workspace_not_allowed",
            Self::WorkspaceLimitExceeded => "workspace_limit_exceeded",
            Self::IndexInProgress => "index_in_progress",
            Self::IndexNotReady => "index_not_ready",
            Self::SyncInProgress => "sync_in_progress",
            Self::IndexStale => "index_stale",
            Self::IndexIncompatible => "index_incompatible",
            Self::RefNotIndexed => "ref_not_indexed",
            Self::OverlayNotReady => "overlay_not_ready",
            Self::MergeBaseFailed => "merge_base_failed",
            Self::SymbolNotFound => "symbol_not_found",
            Self::AmbiguousSymbol => "ambiguous_symbol",
            Self::FileNotFound => "file_not_found",
            Self::ResultNotFound => "result_not_found",
            Self::NoEdgesAvailable => "no_edges_available",
            Self::InternalError => "internal_error",
        }
    }
}

impl std::fmt::Display for ProtocolErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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

    #[error("vcs error: {0}")]
    Vcs(String),

    #[error("sync in progress: project_id={project_id}, ref={ref_name}, job_id={job_id}")]
    SyncInProgress {
        project_id: String,
        ref_name: String,
        job_id: String,
    },

    #[error("maintenance lock busy: operation={operation}, lock_path={lock_path}")]
    MaintenanceLockBusy {
        operation: String,
        lock_path: String,
    },

    #[error("ref not indexed: project_id={project_id}, ref={ref_name}")]
    RefNotIndexed {
        project_id: String,
        ref_name: String,
    },

    #[error("overlay not ready: project_id={project_id}, ref={ref_name}, reason={reason}")]
    OverlayNotReady {
        project_id: String,
        ref_name: String,
        reason: String,
    },

    #[error("merge base failed: base_ref={base_ref}, head_ref={head_ref}, reason={reason}")]
    MergeBaseFailed {
        base_ref: String,
        head_ref: String,
        reason: String,
    },

    #[error("result not found: path={path}, line_start={line_start}")]
    ResultNotFound { path: String, line_start: u32 },

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

    /// Convenience constructor for VCS errors — use with `.map_err(StateError::vcs)`.
    pub fn vcs<E: std::fmt::Display>(e: E) -> Self {
        Self::Vcs(e.to_string())
    }

    pub fn sync_in_progress(
        project_id: impl Into<String>,
        ref_name: impl Into<String>,
        job_id: impl Into<String>,
    ) -> Self {
        Self::SyncInProgress {
            project_id: project_id.into(),
            ref_name: ref_name.into(),
            job_id: job_id.into(),
        }
    }

    pub fn ref_not_indexed(project_id: impl Into<String>, ref_name: impl Into<String>) -> Self {
        Self::RefNotIndexed {
            project_id: project_id.into(),
            ref_name: ref_name.into(),
        }
    }

    pub fn overlay_not_ready(
        project_id: impl Into<String>,
        ref_name: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::OverlayNotReady {
            project_id: project_id.into(),
            ref_name: ref_name.into(),
            reason: reason.into(),
        }
    }

    pub fn merge_base_failed(
        base_ref: impl Into<String>,
        head_ref: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::MergeBaseFailed {
            base_ref: base_ref.into(),
            head_ref: head_ref.into(),
            reason: reason.into(),
        }
    }

    pub fn result_not_found(path: impl Into<String>, line_start: u32) -> Self {
        Self::ResultNotFound {
            path: path.into(),
            line_start,
        }
    }

    pub fn maintenance_lock_busy(
        operation: impl Into<String>,
        lock_path: impl Into<String>,
    ) -> Self {
        Self::MaintenanceLockBusy {
            operation: operation.into(),
            lock_path: lock_path.into(),
        }
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

#[cfg(test)]
mod tests {
    use super::ProtocolErrorCode;

    #[test]
    fn protocol_error_code_strings_match_registry() {
        assert_eq!(ProtocolErrorCode::InvalidInput.as_str(), "invalid_input");
        assert_eq!(
            ProtocolErrorCode::WorkspaceNotAllowed.as_str(),
            "workspace_not_allowed"
        );
        assert_eq!(
            ProtocolErrorCode::IndexIncompatible.as_str(),
            "index_incompatible"
        );
        assert_eq!(
            ProtocolErrorCode::SymbolNotFound.as_str(),
            "symbol_not_found"
        );
        assert_eq!(ProtocolErrorCode::InternalError.as_str(), "internal_error");
    }
}
