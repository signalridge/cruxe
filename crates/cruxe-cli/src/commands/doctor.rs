use anyhow::{Context, Result};
use cruxe_core::config::Config;
use cruxe_core::constants;
use cruxe_core::types::generate_project_id;
use cruxe_indexer::parser;
use cruxe_state::{db, project};
use std::path::Path;

pub fn run(repo_root: &Path, config_file: Option<&Path>) -> Result<()> {
    let repo_root = std::fs::canonicalize(repo_root).context("Failed to resolve project path")?;
    let repo_root_str = repo_root.to_string_lossy().to_string();

    let config = Config::load_with_file(Some(&repo_root), config_file)?;
    let project_id = generate_project_id(&repo_root_str);
    let data_dir = config.project_data_dir(&project_id);

    println!("Cruxe Doctor");
    println!("==================");
    println!();

    let mut all_ok = true;

    // Check SQLite
    let db_path = data_dir.join(constants::STATE_DB_FILE);
    print!("SQLite database ... ");
    if db_path.exists() {
        match db::open_connection(&db_path) {
            Ok(conn) => {
                // Run integrity check
                match conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)) {
                    Ok(result) if result == "ok" => println!("OK"),
                    Ok(result) => {
                        println!("WARN: {}", result);
                        all_ok = false;
                    }
                    Err(e) => {
                        println!("FAIL: {}", e);
                        all_ok = false;
                    }
                }

                // Check project exists
                print!("Project registration ... ");
                match project::get_by_root(&conn, &repo_root_str) {
                    Ok(Some(proj)) => {
                        println!("OK (id: {}, vcs: {})", proj.project_id, proj.vcs_mode);
                    }
                    Ok(None) => {
                        println!("MISSING - run `cruxe init`");
                        all_ok = false;
                    }
                    Err(e) => {
                        println!("FAIL: {}", e);
                        all_ok = false;
                    }
                }
            }
            Err(e) => {
                println!("FAIL: {}", e);
                all_ok = false;
            }
        }
    } else {
        println!("MISSING - run `cruxe init`");
        all_ok = false;
    }

    // Check Tantivy indices
    let base_dir = data_dir.join("base");
    for index_name in &["symbols", "snippets", "files"] {
        let index_dir = base_dir.join(index_name);
        print!("Tantivy index ({}) ... ", index_name);
        if index_dir.exists() {
            match tantivy::Index::open_in_dir(&index_dir) {
                Ok(_) => println!("OK"),
                Err(e) => {
                    println!("FAIL: {} - run `cruxe index --force`", e);
                    all_ok = false;
                }
            }
        } else {
            println!("MISSING - run `cruxe init`");
            all_ok = false;
        }
    }

    // Check tree-sitter grammars
    println!();
    println!("Tree-sitter grammars:");
    for lang in parser::supported_languages() {
        print!("  {} ... ", lang);
        if parser::is_language_supported(lang) {
            // Try to actually create a parser with the grammar
            match parser::get_language(lang) {
                Ok(_) => println!("OK"),
                Err(e) => {
                    println!("FAIL: {}", e);
                    all_ok = false;
                }
            }
        } else {
            println!("NOT AVAILABLE");
        }
    }

    // Check ignore rules
    println!();
    let ignore_file = repo_root.join(constants::IGNORE_FILE);
    print!(".cruxeignore ... ");
    if ignore_file.exists() {
        match std::fs::read_to_string(&ignore_file) {
            Ok(content) => {
                let rules = content
                    .lines()
                    .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                    .count();
                println!("OK ({} rules)", rules);
            }
            Err(e) => println!("WARN: cannot read: {}", e),
        }
    } else {
        println!("not present (using defaults only)");
    }

    println!();
    if all_ok {
        println!("All checks passed!");
    } else {
        println!("Some checks failed. See above for details.");
    }

    Ok(())
}
