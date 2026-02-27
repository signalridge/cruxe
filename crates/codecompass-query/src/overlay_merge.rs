use crate::locate::LocateResult;
use crate::search::SearchResult;
use codecompass_core::types::{OverlayMergeKey, SourceLayer};
use std::collections::{HashMap, HashSet};

fn annotate_source_layer<T, F>(mut items: Vec<T>, layer: SourceLayer, mut set_layer: F) -> Vec<T>
where
    F: FnMut(&mut T, SourceLayer),
{
    for item in &mut items {
        set_layer(item, layer);
    }
    items
}

/// Extract canonical merge key for search results.
///
/// - symbols: `repo + symbol_stable_id + kind` when available
/// - snippets: `repo + path + chunk_type + line_start + line_end`
/// - files: `repo + path`
/// - fallback: `repo + result_type + path + line_start + line_end`
pub fn search_merge_key(result: &SearchResult) -> OverlayMergeKey {
    if result.result_type == "symbol"
        && let (Some(stable_id), Some(kind)) = (&result.symbol_stable_id, &result.kind)
        && !stable_id.is_empty()
        && !kind.is_empty()
    {
        return OverlayMergeKey::symbol(&result.repo, stable_id, kind);
    }

    match result.result_type.as_str() {
        "snippet" => OverlayMergeKey::snippet(
            &result.repo,
            &result.path,
            result.chunk_type.as_deref().unwrap_or("unknown"),
            result.line_start,
            result.line_end,
        ),
        "file" => OverlayMergeKey::file(&result.repo, &result.path),
        _ => OverlayMergeKey::fallback(
            &result.repo,
            &result.result_type,
            &result.path,
            result.line_start,
            result.line_end,
        ),
    }
}

/// Extract canonical merge key for locate results.
pub fn locate_merge_key(result: &LocateResult) -> OverlayMergeKey {
    if !result.symbol_stable_id.is_empty() && !result.kind.is_empty() {
        return OverlayMergeKey::symbol(&result.repo, &result.symbol_stable_id, &result.kind);
    }
    OverlayMergeKey::fallback(
        &result.repo,
        "symbol-fallback",
        &result.path,
        result.line_start,
        result.line_end,
    )
}

/// Merge base and overlay search results.
///
/// Behavior:
/// - annotate `source_layer` (`base` / `overlay`)
/// - suppress tombstoned base paths
/// - dedupe by merge key with overlay-wins precedence
/// - sort by score descending
pub fn merged_search(
    base_results: Vec<SearchResult>,
    overlay_results: Vec<SearchResult>,
    tombstones: &HashSet<String>,
) -> Vec<SearchResult> {
    let base_results = annotate_source_layer(base_results, SourceLayer::Base, |item, layer| {
        item.source_layer = Some(layer);
    });
    let overlay_results =
        annotate_source_layer(overlay_results, SourceLayer::Overlay, |item, layer| {
            item.source_layer = Some(layer);
        });

    // Pre-compute overlay merge keys so we can preserve base results at a
    // tombstoned path if the overlay explicitly re-provides that merge key
    // (e.g. overlay edited a file but added back a different symbol).
    let overlay_keys: HashSet<OverlayMergeKey> =
        overlay_results.iter().map(search_merge_key).collect();

    let mut merged: HashMap<OverlayMergeKey, SearchResult> = HashMap::new();
    for result in base_results.into_iter() {
        let key = search_merge_key(&result);
        if !tombstones.contains(result.path.as_str()) || overlay_keys.contains(&key) {
            merged.insert(key, result);
        }
    }
    // Overlay-wins: overlay results overwrite any base entry with the same key.
    for result in overlay_results {
        merged.insert(search_merge_key(&result), result);
    }

    let mut out: Vec<SearchResult> = merged.into_values().collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// Merge base and overlay locate results using overlay-wins precedence.
pub fn merged_locate(
    base_results: Vec<LocateResult>,
    overlay_results: Vec<LocateResult>,
    tombstones: &HashSet<String>,
) -> Vec<LocateResult> {
    let base_results = annotate_source_layer(base_results, SourceLayer::Base, |item, layer| {
        item.source_layer = Some(layer);
    });
    let overlay_results =
        annotate_source_layer(overlay_results, SourceLayer::Overlay, |item, layer| {
            item.source_layer = Some(layer);
        });

    // Pre-compute overlay merge keys so we can preserve base results at a
    // tombstoned path if the overlay explicitly re-provides that merge key
    // (consistent with merged_search behavior).
    let overlay_keys: HashSet<OverlayMergeKey> =
        overlay_results.iter().map(locate_merge_key).collect();

    let mut merged: HashMap<OverlayMergeKey, LocateResult> = HashMap::new();
    for result in base_results.into_iter() {
        let key = locate_merge_key(&result);
        if !tombstones.contains(result.path.as_str()) || overlay_keys.contains(&key) {
            merged.insert(key, result);
        }
    }
    for result in overlay_results {
        merged.insert(locate_merge_key(&result), result);
    }

    let mut out: Vec<LocateResult> = merged.into_values().collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_result(
        result_type: &str,
        path: &str,
        line_start: u32,
        line_end: u32,
        score: f32,
    ) -> SearchResult {
        SearchResult {
            repo: "repo-1".to_string(),
            result_id: format!("{result_type}:{path}:{line_start}:{line_end}"),
            symbol_id: None,
            symbol_stable_id: None,
            result_type: result_type.to_string(),
            path: path.to_string(),
            line_start,
            line_end,
            kind: None,
            name: None,
            qualified_name: None,
            language: "rust".to_string(),
            signature: None,
            visibility: None,
            score,
            snippet: None,
            chunk_type: (result_type == "snippet").then(|| "symbol_body".to_string()),
            source_layer: None,
            provenance: "lexical".to_string(),
        }
    }

    fn locate_result(path: &str, stable_id: &str, kind: &str, score: f32) -> LocateResult {
        LocateResult {
            repo: "repo-1".to_string(),
            symbol_id: format!("sym:{stable_id}"),
            symbol_stable_id: stable_id.to_string(),
            path: path.to_string(),
            line_start: 10,
            line_end: 20,
            kind: kind.to_string(),
            name: "run".to_string(),
            qualified_name: "mod::run".to_string(),
            signature: None,
            language: "rust".to_string(),
            visibility: None,
            source_layer: None,
            score,
        }
    }

    #[test]
    fn merged_search_applies_tombstones_and_overlay_wins() {
        let mut base_symbol = search_result("symbol", "src/lib.rs", 10, 20, 0.8);
        base_symbol.symbol_stable_id = Some("stable-1".to_string());
        base_symbol.kind = Some("function".to_string());

        let mut overlay_symbol = search_result("symbol", "src/lib.rs", 10, 20, 0.95);
        overlay_symbol.symbol_stable_id = Some("stable-1".to_string());
        overlay_symbol.kind = Some("function".to_string());

        let tombstoned_base = search_result("file", "src/deleted.rs", 0, 0, 0.9);
        let overlay_only = search_result("file", "src/new.rs", 0, 0, 0.7);

        let merged = merged_search(
            vec![base_symbol, tombstoned_base],
            vec![overlay_symbol.clone(), overlay_only.clone()],
            &HashSet::from(["src/deleted.rs".to_string()]),
        );

        assert_eq!(merged.len(), 2);
        assert!(merged.iter().all(|r| r.path != "src/deleted.rs"));
        let overlay_winner = merged.iter().find(|r| r.path == "src/lib.rs").unwrap();
        assert_eq!(overlay_winner.source_layer, Some(SourceLayer::Overlay));
        assert!((overlay_winner.score - overlay_symbol.score).abs() < f32::EPSILON);
    }

    #[test]
    fn merged_locate_tombstone_reprovision_preserves_overlay_winner() {
        // Base has a symbol at a tombstoned path. Overlay re-provides the same
        // merge key (same stable_id+kind). The overlay version should survive.
        let base = locate_result("src/deleted.rs", "stable-1", "function", 0.4);
        let overlay = locate_result("src/deleted.rs", "stable-1", "function", 0.9);

        let merged = merged_locate(
            vec![base],
            vec![overlay.clone()],
            &HashSet::from(["src/deleted.rs".to_string()]),
        );
        assert_eq!(merged.len(), 1);
        let winner = &merged[0];
        assert_eq!(winner.source_layer, Some(SourceLayer::Overlay));
        assert!((winner.score - overlay.score).abs() < f32::EPSILON);
    }

    #[test]
    fn merged_locate_tombstone_without_reprovision_suppresses_base() {
        // Base has a symbol at a tombstoned path. Overlay does NOT re-provide
        // the same merge key. The base result should be suppressed.
        let base = locate_result("src/deleted.rs", "stable-1", "function", 0.4);
        let overlay = locate_result("src/new.rs", "stable-2", "function", 0.9);

        let merged = merged_locate(
            vec![base],
            vec![overlay],
            &HashSet::from(["src/deleted.rs".to_string()]),
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].path, "src/new.rs");
    }

    #[test]
    fn merged_locate_uses_symbol_stable_id_key() {
        let base = locate_result("src/lib.rs", "stable-1", "function", 0.4);
        let overlay = locate_result("src/lib.rs", "stable-1", "function", 0.9);
        let extra = locate_result("src/new.rs", "stable-2", "function", 0.5);

        let merged = merged_locate(vec![base], vec![overlay.clone(), extra], &HashSet::new());
        assert_eq!(merged.len(), 2);
        let winner = merged
            .iter()
            .find(|r| r.symbol_stable_id == "stable-1")
            .unwrap();
        assert_eq!(winner.source_layer, Some(SourceLayer::Overlay));
        assert!((winner.score - overlay.score).abs() < f32::EPSILON);
    }
}
