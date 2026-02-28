use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticAdvisorInput {
    pub file_count: usize,
    pub language_counts: BTreeMap<String, usize>,
    pub target_latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticAdvisorRecommendation {
    pub profile: String,
    pub repo_size_bucket: String,
    pub reason_codes: Vec<String>,
}

pub fn recommend_semantic_profile(input: &SemanticAdvisorInput) -> SemanticAdvisorRecommendation {
    let repo_size_bucket = if input.file_count < 10_000 {
        "<10k".to_string()
    } else if input.file_count <= 50_000 {
        "10k-50k".to_string()
    } else {
        ">50k".to_string()
    };

    let total_language_files: usize = input.language_counts.values().sum();
    let code_language_weight: usize = input
        .language_counts
        .iter()
        .filter(|(language, _)| cruxe_core::languages::is_semantic_code_language(language))
        .map(|(_, count)| *count)
        .sum();
    let code_mix_ratio = if total_language_files == 0 {
        0.0
    } else {
        code_language_weight as f64 / total_language_files as f64
    };

    let mut reason_codes = vec![
        format!("repo_bucket:{repo_size_bucket}"),
        format!("latency_budget:{}ms", input.target_latency_ms),
        format!("code_mix:{:.2}", code_mix_ratio),
    ];

    let profile = if input.target_latency_ms <= 180 || input.file_count > 50_000 {
        reason_codes.push("prefer_low_latency".to_string());
        "fast_local"
    } else if input.file_count < 10_000 && input.target_latency_ms >= 650 && code_mix_ratio >= 0.6 {
        reason_codes.push("small_repo_high_budget".to_string());
        "high_quality"
    } else if code_mix_ratio >= 0.45 && input.target_latency_ms >= 260 {
        reason_codes.push("balanced_quality_latency".to_string());
        "code_quality"
    } else if code_mix_ratio >= 0.3 {
        // Moderate code mix with mid-latency budget (181..259ms).
        reason_codes.push("moderate_latency_code_mix".to_string());
        "fast_local"
    } else {
        reason_codes.push("fallback_fast_local".to_string());
        "fast_local"
    };

    SemanticAdvisorRecommendation {
        profile: profile.to_string(),
        repo_size_bucket,
        reason_codes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn advisor_prefers_fast_local_for_large_repo_or_tight_budget() {
        let recommendation = recommend_semantic_profile(&SemanticAdvisorInput {
            file_count: 75_000,
            language_counts: BTreeMap::from([
                ("rust".to_string(), 10_000),
                ("typescript".to_string(), 8_000),
            ]),
            target_latency_ms: 250,
        });
        assert_eq!(recommendation.profile, "fast_local");
        assert_eq!(recommendation.repo_size_bucket, ">50k");
    }

    #[test]
    fn advisor_prefers_code_quality_for_balanced_workload() {
        let recommendation = recommend_semantic_profile(&SemanticAdvisorInput {
            file_count: 22_000,
            language_counts: BTreeMap::from([
                ("rust".to_string(), 4_000),
                ("typescript".to_string(), 3_000),
                ("markdown".to_string(), 1_000),
            ]),
            target_latency_ms: 400,
        });
        assert_eq!(recommendation.profile, "code_quality");
        assert_eq!(recommendation.repo_size_bucket, "10k-50k");
    }

    #[test]
    fn advisor_recommendation_is_deterministic_for_same_snapshot() {
        let input = SemanticAdvisorInput {
            file_count: 8_500,
            language_counts: BTreeMap::from([
                ("go".to_string(), 2_200),
                ("python".to_string(), 1_300),
                ("yaml".to_string(), 400),
            ]),
            target_latency_ms: 700,
        };

        let first = recommend_semantic_profile(&input);
        let second = recommend_semantic_profile(&input);
        let third = recommend_semantic_profile(&input);
        assert_eq!(first, second);
        assert_eq!(second, third);
    }
}
