use cruxe_core::constants;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing::{debug, warn};

/// A discovered source file with its detected language.
#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub relative_path: String,
    pub language: String,
}

/// Built-in default ignore patterns for binary/generated files.
const BUILTIN_IGNORE_EXTENSIONS: &[&str] = &[
    ".exe", ".dll", ".so", ".dylib", ".o", ".a", ".wasm", ".pyc", ".class", ".jar", ".min.js",
    ".min.css",
];

const BUILTIN_IGNORE_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".tox",
    "target",
    "build",
    "dist",
    ".next",
    ".nuxt",
    "vendor",
    ".venv",
    "venv",
];

const BUILTIN_IGNORE_PATTERNS: &[&str] = &["*.generated.*", "*.pb.go", "*_generated.rs"];

/// Scan a directory for source files, respecting ignore rules.
/// If `languages` is non-empty, only files matching those language names are included.
pub fn scan_directory(repo_root: &Path, max_file_size: u64) -> Vec<ScannedFile> {
    scan_directory_filtered(repo_root, max_file_size, &[])
}

/// Scan a directory for source files, respecting ignore rules and language filter.
/// If `languages` is non-empty, only files matching those language names are included.
pub fn scan_directory_filtered(
    repo_root: &Path,
    max_file_size: u64,
    languages: &[String],
) -> Vec<ScannedFile> {
    let mut walker = WalkBuilder::new(repo_root);
    walker
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false);

    // Add .cruxeignore
    let ignore_file = repo_root.join(constants::IGNORE_FILE);
    if ignore_file.exists() {
        walker.add_custom_ignore_filename(constants::IGNORE_FILE);
    }

    let mut files = Vec::new();

    for entry in walker.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!("Walk error: {}", e);
                continue;
            }
        };

        let path = entry.path();

        // Skip directories
        if path.is_dir() {
            continue;
        }

        // Skip files matching built-in ignore patterns
        let path_str = path.to_string_lossy();
        if should_ignore_builtin(&path_str) {
            debug!(?path, "Skipped by built-in ignore");
            continue;
        }

        // Check file size
        if let Ok(metadata) = std::fs::metadata(path)
            && metadata.len() > max_file_size
        {
            warn!(?path, size = metadata.len(), "Skipped: file too large");
            continue;
        }

        // Detect language
        if let Some(language) = detect_language(path) {
            // Filter by configured languages (if non-empty)
            if !languages.is_empty() && !languages.iter().any(|l| l == &language) {
                continue;
            }

            let relative = path
                .strip_prefix(repo_root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            files.push(ScannedFile {
                path: path.to_path_buf(),
                relative_path: relative,
                language,
            });
        }
    }

    files
}

fn should_ignore_builtin(path: &str) -> bool {
    let normalized_path = path.replace('\\', "/");

    // Check directory components
    for dir in BUILTIN_IGNORE_DIRS {
        if normalized_path.contains(&format!("/{dir}/")) {
            return true;
        }
    }

    // Check extensions
    for ext in BUILTIN_IGNORE_EXTENSIONS {
        if normalized_path.ends_with(ext) {
            return true;
        }
    }

    builtin_ignore_globset().is_match(&normalized_path)
}

fn builtin_ignore_globset() -> &'static GlobSet {
    static SET: OnceLock<GlobSet> = OnceLock::new();
    SET.get_or_init(|| {
        let mut builder = GlobSetBuilder::new();
        for pattern in BUILTIN_IGNORE_PATTERNS {
            builder.add(Glob::new(pattern).expect("builtin ignore glob pattern must be valid"));
        }
        builder
            .build()
            .expect("builtin ignore glob set must build successfully")
    })
}

/// Detect programming language from file extension.
pub fn detect_language(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    cruxe_core::languages::detect_language_from_extension(ext).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temporary project with source files and return the path.
    fn create_temp_project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create tempdir");
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).expect("create parent dirs");
            }
            fs::write(&full, content).expect("write file");
        }
        dir
    }

    #[test]
    fn test_scan_discovers_supported_languages() {
        let dir = create_temp_project(&[
            ("src/main.rs", "fn main() {}"),
            ("src/lib.py", "def hello(): pass"),
            ("src/app.ts", "function app() {}"),
            ("README.md", "# Readme"),
        ]);

        let files = scan_directory(dir.path(), 1_048_576);
        let langs: Vec<&str> = files.iter().map(|f| f.language.as_str()).collect();

        assert!(langs.contains(&"rust"), "should find rust files");
        assert!(langs.contains(&"python"), "should find python files");
        assert!(
            langs.contains(&"typescript"),
            "should find typescript files"
        );
        // README.md should NOT be included (config/doc file)
        assert!(!files.iter().any(|f| f.relative_path.contains("README")));
    }

    #[test]
    fn test_scan_skips_builtin_ignore_dirs() {
        let dir = create_temp_project(&[
            ("src/main.rs", "fn main() {}"),
            ("node_modules/pkg/index.js", "module.exports = {}"),
            ("target/debug/build.rs", "fn build() {}"),
        ]);

        let files = scan_directory(dir.path(), 1_048_576);
        assert!(
            !files
                .iter()
                .any(|f| f.relative_path.contains("node_modules")),
            "node_modules should be ignored"
        );
        assert!(
            !files.iter().any(|f| f.relative_path.contains("target")),
            "target should be ignored"
        );
    }

    #[test]
    fn test_scan_skips_files_over_max_size() {
        let dir = create_temp_project(&[
            ("small.rs", "fn small() {}"),
            ("large.rs", &"x".repeat(2_000_000)),
        ]);

        let files = scan_directory(dir.path(), 1_048_576);
        assert!(
            files.iter().any(|f| f.relative_path.contains("small.rs")),
            "small file should be included"
        );
        assert!(
            !files.iter().any(|f| f.relative_path.contains("large.rs")),
            "large file should be excluded"
        );
    }

    #[test]
    fn test_cruxeignore_basic_patterns() {
        let dir = create_temp_project(&[
            ("src/main.rs", "fn main() {}"),
            ("src/generated.rs", "// generated"),
            ("tests/test.rs", "// test"),
            (".cruxeignore", "tests/\n"),
        ]);

        let files = scan_directory(dir.path(), 1_048_576);
        assert!(
            files.iter().any(|f| f.relative_path.contains("main.rs")),
            "main.rs should be included"
        );
        assert!(
            !files.iter().any(|f| f.relative_path.contains("test.rs")),
            "tests/ should be ignored by .cruxeignore"
        );
    }

    #[test]
    fn test_cruxeignore_negation_pattern() {
        // .cruxeignore with negation: ignore all files in tests/ except tests/important.rs
        // NOTE: must use `tests/*` (glob) not `tests/` (dir exclude), because directory
        // exclusion prevents descending into the directory entirely, so negation can't
        // un-ignore files inside. This matches standard .gitignore semantics.
        let dir = create_temp_project(&[
            ("src/main.rs", "fn main() {}"),
            ("tests/unit.rs", "// unit test"),
            ("tests/important.rs", "// important test"),
            (".cruxeignore", "tests/*\n!tests/important.rs\n"),
        ]);

        let files = scan_directory(dir.path(), 1_048_576);
        let paths: Vec<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();

        assert!(
            paths.iter().any(|p| p.contains("main.rs")),
            "main.rs should be included"
        );
        assert!(
            !paths.iter().any(|p| p.contains("unit.rs")),
            "tests/unit.rs should be ignored"
        );
        assert!(
            paths.iter().any(|p| p.contains("important.rs")),
            "tests/important.rs should be UN-ignored by negation pattern; found: {:?}",
            paths
        );
    }

    #[test]
    fn test_scan_filtered_by_languages() {
        let dir = create_temp_project(&[
            ("src/main.rs", "fn main() {}"),
            ("src/lib.py", "def hello(): pass"),
            ("src/app.ts", "function app() {}"),
            ("src/server.go", "package main"),
        ]);

        // Only index rust and python
        let languages = vec!["rust".to_string(), "python".to_string()];
        let files = scan_directory_filtered(dir.path(), 1_048_576, &languages);
        let langs: Vec<&str> = files.iter().map(|f| f.language.as_str()).collect();

        assert!(langs.contains(&"rust"), "rust should be included");
        assert!(langs.contains(&"python"), "python should be included");
        assert!(
            !langs.contains(&"typescript"),
            "typescript should be excluded"
        );
        assert!(!langs.contains(&"go"), "go should be excluded");
    }

    #[test]
    fn test_scan_empty_languages_includes_all() {
        let dir = create_temp_project(&[
            ("src/main.rs", "fn main() {}"),
            ("src/app.ts", "function app() {}"),
        ]);

        // Empty language list means include all
        let files = scan_directory_filtered(dir.path(), 1_048_576, &[]);
        assert!(files.len() >= 2, "all supported files should be included");
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language(Path::new("foo.rs")), Some("rust".into()));
        assert_eq!(detect_language(Path::new("foo.py")), Some("python".into()));
        assert_eq!(
            detect_language(Path::new("foo.ts")),
            Some("typescript".into())
        );
        assert_eq!(detect_language(Path::new("foo.go")), Some("go".into()));
        assert_eq!(detect_language(Path::new("foo.toml")), None);
        assert_eq!(detect_language(Path::new("foo.md")), None);
        assert_eq!(detect_language(Path::new("foo")), None);
    }
}
