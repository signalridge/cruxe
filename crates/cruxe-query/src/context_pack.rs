use crate::search::{self, SearchResult};
use cruxe_core::error::StateError;
use cruxe_core::tokens::estimate_tokens;
use cruxe_state::manifest;
use cruxe_state::tantivy_index::IndexSet;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;
use thiserror::Error;

mod sectioning;
use sectioning::assign_section;

pub const DEFAULT_MAX_CANDIDATES: usize = 72;
const MAX_SNIPPET_LINES: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackMode {
    Full,
    EditMinimal,
}

impl ContextPackMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::EditMinimal => "edit_minimal",
        }
    }
}

pub fn parse_mode(value: Option<&str>) -> Result<ContextPackMode, ContextPackError> {
    match value.unwrap_or("full") {
        "full" => Ok(ContextPackMode::Full),
        "edit_minimal" | "aider_minimal" => Ok(ContextPackMode::EditMinimal),
        _ => Err(ContextPackError::InvalidMode),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ContextPackSection {
    Definitions,
    Usages,
    Deps,
    Tests,
    Config,
    Docs,
}

impl ContextPackSection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Definitions => "definitions",
            Self::Usages => "usages",
            Self::Deps => "deps",
            Self::Tests => "tests",
            Self::Config => "config",
            Self::Docs => "docs",
        }
    }
}

const SECTION_ORDER: [ContextPackSection; 6] = [
    ContextPackSection::Definitions,
    ContextPackSection::Usages,
    ContextPackSection::Deps,
    ContextPackSection::Tests,
    ContextPackSection::Config,
    ContextPackSection::Docs,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionCaps {
    pub definitions: usize,
    pub usages: usize,
    pub deps: usize,
    pub tests: usize,
    pub config: usize,
    pub docs: usize,
}

impl SectionCaps {
    pub fn defaults(mode: ContextPackMode) -> Self {
        match mode {
            ContextPackMode::Full => Self {
                definitions: 8,
                usages: 8,
                deps: 5,
                tests: 4,
                config: 4,
                docs: 3,
            },
            ContextPackMode::EditMinimal => Self {
                definitions: 8,
                usages: 6,
                deps: 4,
                tests: 1,
                config: 2,
                docs: 0,
            },
        }
    }

    fn cap_for(&self, section: ContextPackSection) -> usize {
        match section {
            ContextPackSection::Definitions => self.definitions,
            ContextPackSection::Usages => self.usages,
            ContextPackSection::Deps => self.deps,
            ContextPackSection::Tests => self.tests,
            ContextPackSection::Config => self.config,
            ContextPackSection::Docs => self.docs,
        }
    }

    pub fn with_patch(mut self, patch: SectionCapsPatch) -> Self {
        if let Some(definitions) = patch.definitions {
            self.definitions = definitions;
        }
        if let Some(usages) = patch.usages {
            self.usages = usages;
        }
        if let Some(deps) = patch.deps {
            self.deps = deps;
        }
        if let Some(tests) = patch.tests {
            self.tests = tests;
        }
        if let Some(config) = patch.config {
            self.config = config;
        }
        if let Some(docs) = patch.docs {
            self.docs = docs;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SectionCapsPatch {
    #[serde(default)]
    pub definitions: Option<usize>,
    #[serde(default, alias = "key_usages")]
    pub usages: Option<usize>,
    #[serde(default, alias = "dependencies")]
    pub deps: Option<usize>,
    #[serde(default)]
    pub tests: Option<usize>,
    #[serde(default)]
    pub config: Option<usize>,
    #[serde(default)]
    pub docs: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackItem {
    pub snippet_id: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub content_hash: String,
    pub selection_reason: String,
    pub score: f64,
    pub estimated_tokens: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_stable_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextPackSections {
    pub definitions: Vec<ContextPackItem>,
    pub usages: Vec<ContextPackItem>,
    pub deps: Vec<ContextPackItem>,
    pub tests: Vec<ContextPackItem>,
    pub config: Vec<ContextPackItem>,
    pub docs: Vec<ContextPackItem>,
}

impl ContextPackSections {
    fn section_mut(&mut self, section: ContextPackSection) -> &mut Vec<ContextPackItem> {
        match section {
            ContextPackSection::Definitions => &mut self.definitions,
            ContextPackSection::Usages => &mut self.usages,
            ContextPackSection::Deps => &mut self.deps,
            ContextPackSection::Tests => &mut self.tests,
            ContextPackSection::Config => &mut self.config,
            ContextPackSection::Docs => &mut self.docs,
        }
    }

    fn section(&self, section: ContextPackSection) -> &[ContextPackItem] {
        match section {
            ContextPackSection::Definitions => &self.definitions,
            ContextPackSection::Usages => &self.usages,
            ContextPackSection::Deps => &self.deps,
            ContextPackSection::Tests => &self.tests,
            ContextPackSection::Config => &self.config,
            ContextPackSection::Docs => &self.docs,
        }
    }

    fn total_items(&self) -> usize {
        self.definitions.len()
            + self.usages.len()
            + self.deps.len()
            + self.tests.len()
            + self.config.len()
            + self.docs.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageSummary {
    pub section_counts: BTreeMap<String, usize>,
    pub section_tokens: BTreeMap<String, usize>,
    pub missing_sections: Vec<String>,
    pub duplicate_candidates: usize,
    pub overflow_candidates: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildContextPackResponse {
    pub query: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub mode: ContextPackMode,
    pub budget_tokens: usize,
    pub token_budget_used: usize,
    pub dropped_candidates: usize,
    pub sections: ContextPackSections,
    pub coverage_summary: CoverageSummary,
    pub suggested_next_queries: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_context_hints: Vec<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum ContextPackError {
    #[error("invalid query")]
    InvalidQuery,
    #[error("invalid budget_tokens")]
    InvalidBudgetTokens,
    #[error("invalid mode")]
    InvalidMode,
    #[error("state error: {0}")]
    State(#[from] StateError),
}

pub struct BuildContextPackParams<'a> {
    pub index_set: &'a IndexSet,
    pub conn: Option<&'a Connection>,
    pub workspace: &'a Path,
    pub query: &'a str,
    pub ref_name: Option<&'a str>,
    pub language: Option<&'a str>,
    pub budget_tokens: usize,
    pub max_candidates: usize,
    pub mode: ContextPackMode,
    pub section_caps: Option<SectionCaps>,
}

#[derive(Clone)]
struct Candidate {
    probe_rank: usize,
    section: ContextPackSection,
    result: SearchResult,
    snippet: Option<String>,
    content_hash: String,
    selection_reason: String,
    estimated_tokens: usize,
    duplicate_count: usize,
}

pub fn build_context_pack(
    params: BuildContextPackParams<'_>,
) -> Result<BuildContextPackResponse, ContextPackError> {
    let BuildContextPackParams {
        index_set,
        conn,
        workspace,
        query,
        ref_name,
        language,
        budget_tokens,
        max_candidates,
        mode,
        section_caps,
    } = params;

    let query = query.trim();
    if query.is_empty() {
        return Err(ContextPackError::InvalidQuery);
    }
    if budget_tokens == 0 {
        return Err(ContextPackError::InvalidBudgetTokens);
    }

    let effective_ref = ref_name.unwrap_or("live").to_string();
    let caps = section_caps.unwrap_or_else(|| SectionCaps::defaults(mode));
    let probes = probe_queries(query, mode);
    let max_candidates = max_candidates.max(1);
    let per_probe_limit = max_candidates.div_ceil(probes.len()).max(1);
    let mut source_cache: BTreeMap<(String, String), Option<String>> = BTreeMap::new();

    let mut raw_candidates = Vec::new();
    for (probe_rank, (label, probe_query)) in probes.iter().enumerate() {
        let response = search::search_code(
            index_set,
            conn,
            probe_query,
            ref_name,
            language,
            per_probe_limit,
            false,
        )?;

        for result in response.results {
            let (section, rule_reason) = assign_section(&result);
            let snippet = resolve_snippet(
                workspace,
                &effective_ref,
                &result.path,
                result.line_start,
                result.line_end,
                &mut source_cache,
                result.snippet.as_deref(),
            );
            let content_hash =
                compute_content_hash(conn, &effective_ref, &result, snippet.as_deref());
            let selection_reason = format!("{label}:{rule_reason}");
            let estimated_tokens =
                estimate_candidate_tokens(&result, snippet.as_deref(), &selection_reason);
            raw_candidates.push(Candidate {
                probe_rank,
                section,
                result,
                snippet,
                content_hash,
                selection_reason,
                estimated_tokens,
                duplicate_count: 0,
            });
        }
    }

    raw_candidates.sort_by(compare_probe_priority);
    if raw_candidates.len() > max_candidates {
        raw_candidates.truncate(max_candidates);
    }

    let raw_count = raw_candidates.len();
    let (deduped_candidates, duplicate_candidates) = cluster_candidates(raw_candidates);
    Ok(assemble_pack(
        query,
        &effective_ref,
        mode,
        budget_tokens,
        caps,
        deduped_candidates,
        raw_count,
        duplicate_candidates,
        probes,
    ))
}

#[allow(clippy::too_many_arguments)]
fn assemble_pack(
    query: &str,
    effective_ref: &str,
    mode: ContextPackMode,
    budget_tokens: usize,
    caps: SectionCaps,
    deduped_candidates: Vec<Candidate>,
    raw_candidate_count: usize,
    duplicate_candidates: usize,
    probes: Vec<(String, String)>,
) -> BuildContextPackResponse {
    let mut by_section: BTreeMap<ContextPackSection, Vec<Candidate>> = BTreeMap::new();
    for candidate in deduped_candidates {
        by_section
            .entry(candidate.section)
            .or_default()
            .push(candidate);
    }

    for section in SECTION_ORDER {
        if let Some(bucket) = by_section.get_mut(&section) {
            bucket.sort_by(compare_within_section);
        }
    }

    let mut primary_queue: Vec<(ContextPackSection, Candidate)> = Vec::new();
    let mut overflow_queue: Vec<(ContextPackSection, Candidate)> = Vec::new();
    let mut overflow_candidates = BTreeMap::new();

    for section in SECTION_ORDER {
        let cap = caps.cap_for(section);
        let mut bucket = by_section.remove(&section).unwrap_or_default();
        if cap == 0 {
            if !bucket.is_empty() {
                overflow_candidates.insert(section.as_str().to_string(), bucket.len());
            }
            continue;
        }

        if bucket.len() > cap {
            let overflow = bucket.split_off(cap);
            overflow_candidates.insert(section.as_str().to_string(), overflow.len());
            overflow_queue.extend(overflow.into_iter().map(|candidate| (section, candidate)));
        }

        primary_queue.extend(bucket.into_iter().map(|candidate| (section, candidate)));
    }

    let mut sections = ContextPackSections::default();
    let mut token_budget_used = 0usize;
    let mut dropped_candidates = 0usize;
    let mut dropped_by_budget = BTreeMap::<String, usize>::new();

    for (section, candidate) in primary_queue.into_iter().chain(overflow_queue.into_iter()) {
        if token_budget_used + candidate.estimated_tokens > budget_tokens {
            dropped_candidates += 1;
            *dropped_by_budget
                .entry(section.as_str().to_string())
                .or_insert(0) += 1;
            continue;
        }

        let mut selection_reason = candidate.selection_reason;
        if candidate.duplicate_count > 0 {
            selection_reason = format!(
                "{selection_reason};cluster_size={}",
                candidate.duplicate_count + 1
            );
        }

        let item = ContextPackItem {
            snippet_id: candidate.result.result_id,
            ref_name: effective_ref.to_string(),
            path: candidate.result.path,
            line_start: candidate.result.line_start,
            line_end: candidate.result.line_end,
            content_hash: candidate.content_hash,
            selection_reason,
            score: candidate.result.score as f64,
            estimated_tokens: candidate.estimated_tokens,
            symbol_id: candidate.result.symbol_id,
            symbol_stable_id: candidate.result.symbol_stable_id,
            name: candidate.result.name,
            qualified_name: candidate.result.qualified_name,
            kind: candidate.result.kind,
            language: Some(candidate.result.language).filter(|value| !value.is_empty()),
            snippet: candidate.snippet,
        };
        token_budget_used += item.estimated_tokens;
        sections.section_mut(section).push(item);
    }

    let mut section_counts = BTreeMap::new();
    let mut section_tokens = BTreeMap::new();
    let mut missing_sections = Vec::new();

    for section in SECTION_ORDER {
        let items = sections.section(section);
        section_counts.insert(section.as_str().to_string(), items.len());
        section_tokens.insert(
            section.as_str().to_string(),
            items
                .iter()
                .map(|item| item.estimated_tokens)
                .sum::<usize>(),
        );
        if items.is_empty() && caps.cap_for(section) > 0 {
            missing_sections.push(section.as_str().to_string());
        }
    }

    let selected_candidates = sections.total_items();
    let (suggested_next_queries, missing_context_hints) = build_followup_guidance(
        query,
        &missing_sections,
        dropped_candidates,
        budget_tokens,
        token_budget_used,
        selected_candidates,
        mode,
    );

    let coverage_summary = CoverageSummary {
        section_counts,
        section_tokens,
        missing_sections,
        duplicate_candidates,
        overflow_candidates,
    };

    let budget_utilization_ratio = token_budget_used as f64 / budget_tokens as f64;
    let metadata = json!({
        "total_raw_candidates": raw_candidate_count,
        "deduped_candidates": coverage_summary
            .section_counts
            .values()
            .sum::<usize>()
            + dropped_candidates,
        "selected_candidates": selected_candidates,
        "budget_utilization_ratio": budget_utilization_ratio,
        "token_estimation": {
            "method": "cruxe_core::tokens::estimate_tokens",
            "minimum_per_item": 8
        },
        "mode_aliases": {
            "aider_minimal": "edit_minimal"
        },
        "section_caps": caps,
        "section_aliases": {
            "key_usages": "usages",
            "dependencies": "deps"
        },
        "deterministic_section_order": SECTION_ORDER
            .iter()
            .map(|section| section.as_str())
            .collect::<Vec<_>>(),
        "dropped_by_budget": dropped_by_budget,
        "probes": probes
            .into_iter()
            .map(|(label, query)| json!({
                "label": label,
                "query": query,
            }))
            .collect::<Vec<_>>(),
    });

    BuildContextPackResponse {
        query: query.to_string(),
        ref_name: effective_ref.to_string(),
        mode,
        budget_tokens,
        token_budget_used,
        dropped_candidates,
        sections,
        coverage_summary,
        suggested_next_queries,
        missing_context_hints,
        metadata,
    }
}

fn compare_within_section(left: &Candidate, right: &Candidate) -> Ordering {
    right
        .result
        .score
        .total_cmp(&left.result.score)
        .then_with(|| left.result.path.cmp(&right.result.path))
        .then_with(|| left.result.line_start.cmp(&right.result.line_start))
        .then_with(|| left.result.line_end.cmp(&right.result.line_end))
        .then_with(|| left.result.result_id.cmp(&right.result.result_id))
}

fn compare_probe_priority(left: &Candidate, right: &Candidate) -> Ordering {
    left.probe_rank
        .cmp(&right.probe_rank)
        .then_with(|| right.result.score.total_cmp(&left.result.score))
        .then_with(|| left.result.path.cmp(&right.result.path))
        .then_with(|| left.result.line_start.cmp(&right.result.line_start))
        .then_with(|| left.result.line_end.cmp(&right.result.line_end))
        .then_with(|| left.result.result_id.cmp(&right.result.result_id))
}

fn compare_cluster_preference(left: &Candidate, right: &Candidate) -> Ordering {
    section_rank(left.section)
        .cmp(&section_rank(right.section))
        .then_with(|| right.result.score.total_cmp(&left.result.score))
        .then_with(|| left.result.path.cmp(&right.result.path))
        .then_with(|| left.result.line_start.cmp(&right.result.line_start))
        .then_with(|| left.result.line_end.cmp(&right.result.line_end))
        .then_with(|| left.result.result_id.cmp(&right.result.result_id))
}

fn section_rank(section: ContextPackSection) -> usize {
    SECTION_ORDER
        .iter()
        .position(|candidate| *candidate == section)
        .unwrap_or(usize::MAX)
}

fn cluster_candidates(raw_candidates: Vec<Candidate>) -> (Vec<Candidate>, usize) {
    let mut clusters: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for candidate in raw_candidates {
        clusters
            .entry(dedup_key(&candidate.result))
            .or_default()
            .push(candidate);
    }

    let mut deduped = Vec::new();
    let mut duplicate_candidates = 0usize;

    for (_key, mut candidates) in clusters {
        candidates.sort_by(compare_cluster_preference);
        let mut chosen = candidates.remove(0);
        duplicate_candidates += candidates.len();
        chosen.duplicate_count = candidates.len();
        deduped.push(chosen);
    }

    deduped.sort_by(compare_cluster_preference);
    (deduped, duplicate_candidates)
}

fn dedup_key(result: &SearchResult) -> String {
    if let Some(symbol_stable_id) = result
        .symbol_stable_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return format!(
            "symbol:{symbol_stable_id}:{}:{}:{}",
            result.path, result.line_start, result.line_end
        );
    }
    if let Some(symbol_id) = result
        .symbol_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return format!(
            "symbol:{symbol_id}:{}:{}:{}",
            result.path, result.line_start, result.line_end
        );
    }
    format!(
        "span:{}:{}:{}",
        result.path, result.line_start, result.line_end
    )
}

fn probe_queries(query: &str, mode: ContextPackMode) -> Vec<(String, String)> {
    match mode {
        ContextPackMode::Full => vec![
            ("primary".to_string(), query.to_string()),
            ("tests_probe".to_string(), format!("{query} test")),
            ("deps_probe".to_string(), format!("{query} import")),
        ],
        ContextPackMode::EditMinimal => vec![("primary".to_string(), query.to_string())],
    }
}

fn resolve_snippet(
    workspace: &Path,
    ref_name: &str,
    relative_path: &str,
    line_start: u32,
    line_end: u32,
    source_cache: &mut BTreeMap<(String, String), Option<String>>,
    fallback: Option<&str>,
) -> Option<String> {
    let cache_key = (ref_name.to_string(), relative_path.to_string());
    let cached_content = if let Some(content) = source_cache.get(&cache_key) {
        content.clone()
    } else {
        let loaded = if ref_name.eq_ignore_ascii_case("live") {
            read_source_from_workspace(workspace, relative_path)
        } else {
            read_source_from_git_ref(workspace, ref_name, relative_path)
                .or_else(|| read_source_from_workspace(workspace, relative_path))
        };
        source_cache.insert(cache_key, loaded.clone());
        loaded
    };

    cached_content
        .as_deref()
        .and_then(|content| trim_line_range(content, line_start, line_end))
        .or_else(|| fallback.map(ToOwned::to_owned))
        .and_then(|snippet| {
            let snippet = snippet.trim().to_string();
            if snippet.is_empty() {
                None
            } else {
                Some(snippet)
            }
        })
}

fn read_source_from_workspace(workspace: &Path, relative_path: &str) -> Option<String> {
    let path = workspace.join(relative_path);
    std::fs::read_to_string(path).ok()
}

fn read_source_from_git_ref(
    workspace: &Path,
    ref_name: &str,
    relative_path: &str,
) -> Option<String> {
    if !cruxe_core::vcs::is_git_repo(workspace) {
        return None;
    }
    let object = format!("{ref_name}:{relative_path}");
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(workspace)
        .arg("cat-file")
        .arg("-p")
        .arg("--")
        .arg(object)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok()
}

fn trim_line_range(content: &str, line_start: u32, line_end: u32) -> Option<String> {
    if line_start == 0 || line_end == 0 || line_end < line_start {
        return None;
    }

    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return None;
    }
    let start = line_start.saturating_sub(1) as usize;
    if start >= lines.len() {
        return None;
    }

    let requested_end = (line_end as usize).min(lines.len());
    let capped_end = requested_end.min(start + MAX_SNIPPET_LINES);
    if start >= capped_end {
        return None;
    }

    let mut snippet = lines[start..capped_end].join("\n");
    if requested_end > capped_end {
        snippet.push_str("\n...");
    }
    Some(snippet)
}

fn compute_content_hash(
    conn: Option<&Connection>,
    effective_ref: &str,
    result: &SearchResult,
    snippet: Option<&str>,
) -> String {
    if let Some(snippet) = snippet
        && !snippet.trim().is_empty()
    {
        return blake3::hash(snippet.as_bytes()).to_hex().to_string();
    }

    if let Some(conn) = conn
        && !result.repo.is_empty()
        && let Ok(Some(file_hash)) =
            manifest::get_content_hash(conn, &result.repo, effective_ref, &result.path)
    {
        return file_hash;
    }

    blake3::hash(format!("{}:{}:{}", result.path, result.line_start, result.line_end).as_bytes())
        .to_hex()
        .to_string()
}

fn estimate_candidate_tokens(result: &SearchResult, snippet: Option<&str>, reason: &str) -> usize {
    let descriptor = format!(
        "{} {} {} {} {} {}",
        result.result_type,
        result.path,
        result
            .name
            .as_deref()
            .unwrap_or(result.qualified_name.as_deref().unwrap_or("")),
        result.kind.as_deref().unwrap_or(""),
        reason,
        snippet.unwrap_or("")
    );
    estimate_tokens(&descriptor).max(8)
}

fn build_followup_guidance(
    query: &str,
    missing_sections: &[String],
    dropped_candidates: usize,
    budget_tokens: usize,
    token_budget_used: usize,
    selected_candidates: usize,
    mode: ContextPackMode,
) -> (Vec<String>, Vec<String>) {
    let mut next_queries = Vec::new();
    let mut hints = Vec::new();

    for section in missing_sections {
        match section.as_str() {
            "definitions" => {
                next_queries.push(format!("{query} definition"));
                hints.push("Missing definitions; run a symbol-focused lookup.".to_string());
            }
            "usages" => {
                next_queries.push(format!("{query} call sites"));
                hints.push("Missing usages; gather concrete call/reference spans.".to_string());
            }
            "deps" => {
                next_queries.push(format!("{query} imports"));
                hints
                    .push("Dependency context is thin; collect import/build metadata.".to_string());
            }
            "tests" => {
                next_queries.push(format!("{query} tests"));
                hints
                    .push("No tests selected; request test files for behavior intent.".to_string());
            }
            "config" => {
                next_queries.push(format!("{query} config"));
                hints.push(
                    "No config/build files selected; include runtime/build settings.".to_string(),
                );
            }
            "docs" => {
                next_queries.push(format!("{query} README"));
                hints.push(
                    "No docs selected; add design/user-facing context if needed.".to_string(),
                );
            }
            _ => {}
        }
    }

    if dropped_candidates > 0 {
        next_queries.push(format!("{query} broader context"));
        hints.push(format!(
            "{} candidate(s) were dropped by budget; increase budget_tokens or tighten query.",
            dropped_candidates
        ));
    }
    if selected_candidates == 0 {
        next_queries.push(format!("{query} symbol"));
        hints.push(
            "No candidates were selected; broaden the query or verify index freshness.".to_string(),
        );
    } else if dropped_candidates == 0
        && token_budget_used.saturating_mul(100) < budget_tokens.saturating_mul(40)
    {
        next_queries.push(format!("{query} related symbols"));
        hints.push(format!(
            "Budget underfilled ({token_budget_used}/{budget_tokens} tokens); broaden query or raise max_candidates for richer coverage."
        ));
    }

    if next_queries.is_empty() && matches!(mode, ContextPackMode::EditMinimal) {
        next_queries.push(format!("{query} diff"));
        hints
            .push("Edit-minimal mode favors code spans; ask for diff context when editing.".into());
    }

    next_queries.sort();
    next_queries.dedup();
    hints.sort();
    hints.dedup();

    (next_queries, hints)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchResult;

    fn make_result(
        result_id: &str,
        result_type: &str,
        path: &str,
        line_start: u32,
        line_end: u32,
        score: f32,
    ) -> SearchResult {
        SearchResult {
            repo: "test-repo".to_string(),
            result_id: result_id.to_string(),
            symbol_id: Some(format!("sym-{result_id}")),
            symbol_stable_id: Some(format!("stable-{result_id}")),
            result_type: result_type.to_string(),
            path: path.to_string(),
            line_start,
            line_end,
            kind: Some("function".to_string()),
            name: Some(format!("name_{result_id}")),
            qualified_name: Some(format!("qualified::{result_id}")),
            language: "rust".to_string(),
            signature: Some("fn demo()".to_string()),
            visibility: Some("public".to_string()),
            score,
            snippet: Some("fn demo() { use std::fmt::Debug; }".to_string()),
            chunk_type: Some("function_body".to_string()),
            source_layer: None,
            provenance: "lexical".to_string(),
        }
    }

    fn make_candidate(
        section: ContextPackSection,
        result_id: &str,
        score: f32,
        tokens: usize,
    ) -> Candidate {
        Candidate {
            probe_rank: 0,
            section,
            result: make_result(
                result_id,
                if matches!(section, ContextPackSection::Definitions) {
                    "symbol"
                } else {
                    "snippet"
                },
                "src/lib.rs",
                10,
                20,
                score,
            ),
            snippet: Some("fn demo() {}".to_string()),
            content_hash: format!("hash-{result_id}"),
            selection_reason: "primary:test".to_string(),
            estimated_tokens: tokens,
            duplicate_count: 0,
        }
    }

    #[test]
    fn parse_mode_defaults_and_supports_alias() {
        assert_eq!(parse_mode(None).unwrap(), ContextPackMode::Full);
        assert_eq!(
            parse_mode(Some("aider_minimal")).unwrap(),
            ContextPackMode::EditMinimal
        );
    }

    #[test]
    fn assign_section_applies_priority_order() {
        let mut result = make_result("priority", "symbol", "tests/Cargo.toml", 1, 2, 1.0);
        result.kind = Some("module".to_string());
        let (section, _reason) = assign_section(&result);
        assert_eq!(section, ContextPackSection::Definitions);
    }

    #[test]
    fn assign_section_routes_test_snippet_to_tests_when_no_usage_signal() {
        let mut result = make_result("test-snippet", "snippet", "tests/auth_test.rs", 1, 5, 0.5);
        result.chunk_type = Some("comment".to_string());
        result.snippet = Some("assert_eq!(1, 1);".to_string());

        let (section, _reason) = assign_section(&result);
        assert_eq!(section, ContextPackSection::Tests);
    }

    #[test]
    fn assign_section_routes_test_snippet_to_tests_even_with_usage_signal() {
        let mut result = make_result(
            "test-usage-snippet",
            "snippet",
            "tests/auth_test.rs",
            1,
            5,
            0.5,
        );
        result.chunk_type = Some("call_reference".to_string());
        result.snippet = Some("validate_token call".to_string());

        let (section, _reason) = assign_section(&result);
        assert_eq!(section, ContextPackSection::Tests);
    }

    #[test]
    fn cluster_candidates_dedups_by_symbol_id() {
        let mut first = make_candidate(ContextPackSection::Definitions, "dup", 0.9, 20);
        let mut second = make_candidate(ContextPackSection::Usages, "dup2", 0.8, 20);
        first.result.symbol_stable_id = Some("stable-same".to_string());
        second.result.symbol_stable_id = Some("stable-same".to_string());

        let (deduped, duplicates) = cluster_candidates(vec![first, second]);
        assert_eq!(duplicates, 1);
        assert_eq!(deduped.len(), 1);
        assert!(matches!(
            deduped[0].section,
            ContextPackSection::Definitions
        ));
    }

    #[test]
    fn cluster_candidates_keeps_distinct_spans_for_same_symbol() {
        let mut first = make_candidate(ContextPackSection::Definitions, "span-a", 0.9, 20);
        let mut second = make_candidate(ContextPackSection::Usages, "span-b", 0.8, 20);
        first.result.symbol_stable_id = Some("stable-same".to_string());
        second.result.symbol_stable_id = Some("stable-same".to_string());
        second.result.line_start = 30;
        second.result.line_end = 40;

        let (deduped, duplicates) = cluster_candidates(vec![first, second]);
        assert_eq!(duplicates, 0);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn assemble_pack_is_deterministic() {
        let candidates = vec![
            make_candidate(ContextPackSection::Definitions, "a", 0.9, 18),
            make_candidate(ContextPackSection::Usages, "b", 0.8, 16),
            make_candidate(ContextPackSection::Deps, "c", 0.7, 12),
            make_candidate(ContextPackSection::Tests, "d", 0.6, 12),
            make_candidate(ContextPackSection::Config, "e", 0.5, 12),
        ];
        let caps = SectionCaps::defaults(ContextPackMode::Full);
        let probes = vec![("primary".to_string(), "demo".to_string())];

        let response_a = assemble_pack(
            "demo",
            "main",
            ContextPackMode::Full,
            64,
            caps.clone(),
            candidates.clone(),
            5,
            0,
            probes.clone(),
        );
        let response_b = assemble_pack(
            "demo",
            "main",
            ContextPackMode::Full,
            64,
            caps,
            candidates,
            5,
            0,
            probes,
        );

        assert_eq!(
            serde_json::to_value(response_a).unwrap(),
            serde_json::to_value(response_b).unwrap()
        );
    }

    #[test]
    fn edit_minimal_defaults_drop_docs_section() {
        let caps = SectionCaps::defaults(ContextPackMode::EditMinimal);
        assert_eq!(caps.docs, 0);
        assert!(caps.definitions > 0);
    }

    #[test]
    fn section_caps_patch_overrides_selected_fields_only() {
        let defaults = SectionCaps::defaults(ContextPackMode::Full);
        let patched = defaults.clone().with_patch(SectionCapsPatch {
            definitions: Some(2),
            docs: Some(1),
            ..SectionCapsPatch::default()
        });

        assert_eq!(patched.definitions, 2);
        assert_eq!(patched.docs, 1);
        assert_eq!(patched.usages, defaults.usages);
        assert_eq!(patched.deps, defaults.deps);
        assert_eq!(patched.tests, defaults.tests);
        assert_eq!(patched.config, defaults.config);
    }

    #[test]
    fn section_caps_patch_accepts_long_form_aliases() {
        let patch: SectionCapsPatch = serde_json::from_value(serde_json::json!({
            "key_usages": 3,
            "dependencies": 2
        }))
        .expect("aliases should deserialize");

        assert_eq!(patch.usages, Some(3));
        assert_eq!(patch.deps, Some(2));
    }
}
