mod commands;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "codecompass",
    version,
    about = "Code search and navigation for AI coding assistants",
    long_about = "CodeCompass indexes source code using tree-sitter and Tantivy to provide\n\
        fast symbol location, full-text search, and an MCP server for AI agent integration.\n\n\
        Supported languages: Rust, TypeScript, Python, Go.\n\n\
        Quick start:\n  \
        codecompass init\n  \
        codecompass index\n  \
        codecompass search \"AuthHandler\"\n  \
        codecompass doctor"
)]
struct Cli {
    /// Enable verbose logging (set log level to debug)
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Path to config file (default: .codecompass/config.toml)
    #[arg(long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize CodeCompass for a project
    ///
    /// Creates the SQLite database and Tantivy indices under ~/.codecompass/data/.
    /// Detects VCS mode (git) and registers the project for indexing.
    ///
    /// Example: codecompass init --path /path/to/project
    Init {
        /// Path to the project root (default: current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Check project health and diagnose issues
    ///
    /// Verifies SQLite integrity, Tantivy index accessibility,
    /// tree-sitter grammar availability, and ignore rule configuration.
    ///
    /// Example: codecompass doctor
    Doctor {
        /// Path to the project root (default: current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Index a project's source code
    ///
    /// Scans source files, extracts symbols via tree-sitter, and populates
    /// Tantivy search indices. Uses content hashing for incremental updates.
    ///
    /// Examples:
    ///   codecompass index
    ///   codecompass index --force
    ///   codecompass index --ref feat/auth
    Index {
        /// Path to the project root (default: current directory)
        #[arg(short, long)]
        path: Option<String>,

        /// Force full re-index, ignoring content hashes
        #[arg(long)]
        force: bool,

        /// Ref/branch to index under (default: auto-detect or "live")
        #[arg(long)]
        r#ref: Option<String>,
    },
    /// Search code in the index
    ///
    /// Classifies query intent (symbol, path, error, natural language) and
    /// searches across symbols, snippets, and files indices with ranked results.
    ///
    /// Examples:
    ///   codecompass search "validate_token"
    ///   codecompass search "src/auth/handler.rs"
    ///   codecompass search "connection refused" --lang rust
    ///   codecompass search "AuthHandler" --ref main --limit 5
    Search {
        /// Search query (symbol name, file path, error string, or natural language)
        query: String,

        /// Branch/ref scope (default: auto-detect or "live")
        #[arg(long)]
        r#ref: Option<String>,

        /// Filter by programming language (rust, typescript, python, go)
        #[arg(long)]
        lang: Option<String>,

        /// Maximum number of results to return
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Incremental sync based on file changes
    ///
    /// Detects changed files since last index and updates only those entries.
    /// Equivalent to `codecompass index` without `--force`.
    ///
    /// Examples:
    ///   codecompass sync
    ///   codecompass sync --force
    ///   codecompass sync --workspace /path/to/project
    Sync {
        /// Path to the project root (default: current directory)
        #[arg(long)]
        workspace: Option<String>,

        /// Force full re-index instead of incremental
        #[arg(long)]
        force: bool,
    },
    /// Start MCP server (stdio or HTTP JSON-RPC transport)
    ///
    /// Exposes tools (locate_symbol, search_code, index_status, index_repo,
    /// sync_repo) to AI coding assistants via the Model Context Protocol.
    ///
    /// Examples:
    ///   codecompass serve-mcp --workspace .
    ///   codecompass serve-mcp --transport http --port 9100
    ///   codecompass serve-mcp --auto-workspace --allowed-root /home/user/projects
    ServeMcp {
        /// Path to the default project root (default: current directory)
        #[arg(long)]
        workspace: Option<String>,

        /// Skip Tantivy index prewarming on startup
        #[arg(long)]
        no_prewarm: bool,

        /// Transport mode: "stdio" (default) or "http"
        #[arg(long, default_value = "stdio")]
        transport: String,

        /// HTTP server port (only used with --transport http)
        #[arg(long, default_value = "9100")]
        port: u16,

        /// HTTP server bind address (only used with --transport http)
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,

        /// Enable auto-discovery of workspaces passed via the `workspace` tool parameter.
        /// Requires at least one --allowed-root.
        #[arg(long)]
        auto_workspace: bool,

        /// Allowed root directory prefix for auto-discovered workspaces (repeatable).
        /// Only workspace paths under these roots will be accepted.
        #[arg(long = "allowed-root")]
        allowed_roots: Vec<String>,

        /// Maximum number of auto-discovered workspaces to keep (LRU eviction).
        #[arg(long, default_value = "10")]
        max_auto_workspaces: usize,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Set up tracing
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();

    let config_file = cli.config.as_deref().map(std::path::Path::new);

    match cli.command {
        Commands::Init { path } => {
            let path = resolve_path(path)?;
            commands::init::run(&path, config_file)?;
        }
        Commands::Doctor { path } => {
            let path = resolve_path(path)?;
            commands::doctor::run(&path, config_file)?;
        }
        Commands::Index { path, force, r#ref } => {
            let path = resolve_path(path)?;
            commands::index::run(&path, force, r#ref.as_deref(), config_file)?;
        }
        Commands::Search {
            query,
            r#ref,
            lang,
            limit,
        } => {
            let path = std::env::current_dir()?;
            commands::search::run(
                &path,
                &query,
                r#ref.as_deref(),
                lang.as_deref(),
                limit,
                config_file,
            )?;
        }
        Commands::Sync { workspace, force } => {
            let path = resolve_path(workspace)?;
            commands::index::run(&path, force, None, config_file)?;
        }
        Commands::ServeMcp {
            workspace,
            no_prewarm,
            transport,
            port,
            bind,
            auto_workspace,
            allowed_roots,
            max_auto_workspaces,
        } => {
            let path = resolve_path(workspace)?;
            // HIGH-4: Canonicalize --allowed-root paths at startup; reject nonexistent
            let mut canonical_roots = Vec::new();
            for root in &allowed_roots {
                let root_path = std::path::Path::new(root);
                match root_path.canonicalize() {
                    Ok(canonical) => canonical_roots.push(canonical),
                    Err(e) => {
                        anyhow::bail!("--allowed-root '{}' is not a valid path: {}", root, e);
                    }
                }
            }
            let ws_config = codecompass_core::types::WorkspaceConfig {
                auto_workspace,
                allowed_roots: codecompass_core::types::AllowedRoots::new(canonical_roots),
                max_auto_workspaces,
            };
            match transport.as_str() {
                "http" => {
                    commands::serve_mcp::run_http(
                        &path,
                        config_file,
                        no_prewarm,
                        ws_config,
                        &bind,
                        port,
                    )?;
                }
                _ => {
                    commands::serve_mcp::run(&path, config_file, no_prewarm, ws_config)?;
                }
            }
        }
    }

    Ok(())
}

fn resolve_path(path: Option<String>) -> anyhow::Result<std::path::PathBuf> {
    match path {
        Some(p) => Ok(std::path::PathBuf::from(p)),
        None => Ok(std::env::current_dir()?),
    }
}
