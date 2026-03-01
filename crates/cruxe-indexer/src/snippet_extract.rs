use crate::languages::ExtractedSymbol;
use cruxe_core::config::SemanticChunkingConfig;
use cruxe_core::types::{SnippetRecord, compute_symbol_stable_id};
use tracing::warn;

/// Build SnippetRecords from extracted symbols.
/// Each function/method/class body becomes a snippet.
pub fn build_snippet_records(
    extracted: &[ExtractedSymbol],
    repo: &str,
    r#ref: &str,
    path: &str,
    commit: Option<&str>,
) -> Vec<SnippetRecord> {
    build_snippet_records_with_options(extracted, repo, r#ref, path, commit, None, None, None)
}

#[allow(clippy::too_many_arguments)]
pub fn build_snippet_records_with_options(
    extracted: &[ExtractedSymbol],
    repo: &str,
    r#ref: &str,
    path: &str,
    commit: Option<&str>,
    language: Option<&str>,
    source_content: Option<&str>,
    chunking: Option<&SemanticChunkingConfig>,
) -> Vec<SnippetRecord> {
    let mut snippets: Vec<SnippetRecord> = Vec::new();
    let chunking = chunking.cloned().unwrap_or_default();

    for sym in extracted {
        let Some(body) = sym.body.as_ref() else {
            continue;
        };
        if body.trim().is_empty() {
            continue;
        }

        let chunk_type = match sym.kind {
            cruxe_core::types::SymbolKind::Function => "function_body",
            cruxe_core::types::SymbolKind::Method => "method_body",
            cruxe_core::types::SymbolKind::Class => "class_body",
            cruxe_core::types::SymbolKind::Struct => "struct_body",
            cruxe_core::types::SymbolKind::Trait => "trait_body",
            cruxe_core::types::SymbolKind::Interface => "interface_body",
            cruxe_core::types::SymbolKind::Module => "module_body",
            _ => continue, // Skip constants, variables, etc.
        };

        let parent_symbol_stable_id = compute_symbol_stable_id(
            &sym.language,
            &sym.kind,
            &sym.qualified_name,
            sym.signature.as_deref(),
        );
        let chunks = split_text_with_overlap(body, &chunking);
        for (chunk_index, chunk) in chunks.into_iter().enumerate() {
            let line_start = sym.line_start.saturating_add(chunk.start_line as u32);
            let line_end = sym.line_start.saturating_add(chunk.end_line as u32);
            snippets.push(SnippetRecord {
                repo: repo.to_string(),
                r#ref: r#ref.to_string(),
                commit: commit.map(String::from),
                path: path.to_string(),
                language: sym.language.clone(),
                chunk_type: chunk_type.to_string(),
                origin: "symbol_origin".to_string(),
                parent_symbol_stable_id: Some(parent_symbol_stable_id.clone()),
                chunk_index: chunk_index as u32,
                truncated: chunk.truncated,
                imports: None,
                line_start,
                line_end,
                content: chunk.content,
            });
        }
    }

    // Fallback chunking for files where symbol extraction yields no snippet candidates.
    if snippets.is_empty()
        && let (Some(content), Some(lang)) = (source_content, language)
        && !content.trim().is_empty()
    {
        let chunks = split_text_with_overlap(content, &chunking);
        for (chunk_index, chunk) in chunks.into_iter().enumerate() {
            snippets.push(SnippetRecord {
                repo: repo.to_string(),
                r#ref: r#ref.to_string(),
                commit: commit.map(String::from),
                path: path.to_string(),
                language: lang.to_string(),
                chunk_type: "file_fallback".to_string(),
                origin: "file_fallback".to_string(),
                parent_symbol_stable_id: None,
                chunk_index: chunk_index as u32,
                truncated: chunk.truncated,
                imports: None,
                line_start: chunk.start_line as u32 + 1,
                line_end: chunk.end_line as u32 + 1,
                content: chunk.content,
            });
        }
    }

    snippets
}

#[derive(Debug, Clone)]
struct ChunkedText {
    content: String,
    start_line: usize,
    end_line: usize,
    truncated: bool,
}

fn split_text_with_overlap(text: &str, chunking: &SemanticChunkingConfig) -> Vec<ChunkedText> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    if lines.len() <= chunking.max_chunk_lines {
        let content = lines.join("\n");
        let (content, truncated) = truncate_to_token_budget(&content, chunking.max_chunk_tokens);
        return vec![ChunkedText {
            content,
            start_line: 0,
            end_line: lines.len().saturating_sub(1),
            truncated,
        }];
    }

    let mut chunks = Vec::new();
    let step = chunking
        .chunk_size_lines
        .saturating_sub(chunking.chunk_overlap_lines)
        .max(1);
    let mut start = 0usize;
    while start < lines.len() {
        let end = (start + chunking.chunk_size_lines).min(lines.len());
        let raw_content = lines[start..end].join("\n");
        let (content, truncated) =
            truncate_to_token_budget(&raw_content, chunking.max_chunk_tokens);
        chunks.push(ChunkedText {
            content,
            start_line: start,
            end_line: end.saturating_sub(1),
            truncated,
        });
        if end >= lines.len() {
            break;
        }
        start = start.saturating_add(step);
    }

    chunks
}

fn truncate_to_token_budget(content: &str, max_chunk_tokens: usize) -> (String, bool) {
    let approx_tokens = content.len() / 4;
    if approx_tokens <= max_chunk_tokens {
        return (content.to_string(), false);
    }
    let max_bytes = max_chunk_tokens.saturating_mul(4);
    let mut end = max_bytes.min(content.len());
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = content[..end].to_string();
    warn!(
        original_bytes = content.len(),
        truncated_bytes = truncated.len(),
        max_chunk_tokens,
        "snippet chunk truncated to token budget"
    );
    (truncated, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages::ExtractedSymbol;
    use cruxe_core::types::SymbolKind;

    #[test]
    fn large_symbol_body_is_split_with_overlap() {
        let mut body_lines = Vec::new();
        for idx in 0..120 {
            body_lines.push(format!("let token_{idx} = compute_value_{idx}();"));
        }
        let extracted = vec![ExtractedSymbol {
            name: "demo".to_string(),
            qualified_name: "demo".to_string(),
            kind: SymbolKind::Function,
            language: "rust".to_string(),
            signature: Some("fn demo()".to_string()),
            line_start: 10,
            line_end: 200,
            visibility: Some("pub".to_string()),
            parent_name: None,
            body: Some(body_lines.join("\n")),
        }];

        let snippets = build_snippet_records(&extracted, "repo", "main", "src/lib.rs", None);
        assert!(snippets.len() > 1, "expected overlap-aware chunk splitting");
        assert!(snippets[0].line_start >= 10);
        assert!(
            snippets
                .windows(2)
                .all(|window| window[0].line_start <= window[1].line_start),
            "chunk order should be stable"
        );
    }

    #[test]
    fn fallback_chunks_are_emitted_when_no_symbol_body_exists() {
        let snippets = build_snippet_records_with_options(
            &[],
            "repo",
            "main",
            "README.md",
            None,
            Some("text"),
            Some(
                "This repository documents architecture.\nIt also contains examples.\nFallback chunks should index this text.",
            ),
            None,
        );
        assert!(!snippets.is_empty());
        assert!(
            snippets
                .iter()
                .all(|snippet| snippet.chunk_type == "file_fallback")
        );
        assert!(snippets.iter().all(|snippet| snippet.language == "text"));
    }

    #[test]
    fn small_symbol_body_remains_single_chunk() {
        let extracted = vec![ExtractedSymbol {
            name: "single".to_string(),
            qualified_name: "single".to_string(),
            kind: SymbolKind::Function,
            language: "rust".to_string(),
            signature: Some("fn single()".to_string()),
            line_start: 1,
            line_end: 6,
            visibility: Some("pub".to_string()),
            parent_name: None,
            body: Some("fn single() {\n  let x = 1;\n  let y = x + 1;\n}".to_string()),
        }];

        let snippets = build_snippet_records(&extracted, "repo", "main", "src/lib.rs", None);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].chunk_index, 0);
        assert!(!snippets[0].truncated);
        assert_eq!(snippets[0].origin, "symbol_origin");
        assert!(snippets[0].parent_symbol_stable_id.is_some());
    }

    #[test]
    fn oversized_chunk_sets_truncated_metadata() {
        let mut body_lines = Vec::new();
        for _ in 0..80 {
            body_lines
                .push("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
        }
        let chunking = SemanticChunkingConfig {
            max_chunk_lines: 40,
            chunk_size_lines: 40,
            chunk_overlap_lines: 10,
            max_chunk_tokens: 16,
        };

        let extracted = vec![ExtractedSymbol {
            name: "trunc".to_string(),
            qualified_name: "trunc".to_string(),
            kind: SymbolKind::Function,
            language: "rust".to_string(),
            signature: Some("fn trunc()".to_string()),
            line_start: 1,
            line_end: 120,
            visibility: Some("pub".to_string()),
            parent_name: None,
            body: Some(body_lines.join("\n")),
        }];

        let snippets = build_snippet_records_with_options(
            &extracted,
            "repo",
            "main",
            "src/lib.rs",
            None,
            Some("rust"),
            None,
            Some(&chunking),
        );
        assert!(!snippets.is_empty());
        assert!(snippets.iter().all(|snippet| snippet.truncated));
    }
}
