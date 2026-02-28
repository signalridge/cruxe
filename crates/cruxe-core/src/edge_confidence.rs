use std::borrow::Cow;

pub const CONFIDENCE_HIGH: &str = "high";
pub const CONFIDENCE_MEDIUM: &str = "medium";
pub const CONFIDENCE_LOW: &str = "low";

pub const EDGE_PROVIDER_IMPORT_RESOLVER: &str = "import_resolver";
pub const EDGE_PROVIDER_CALL_RESOLVER: &str = "call_resolver";
pub const EDGE_PROVIDER_LEGACY: &str = "legacy";

pub const RESOLUTION_RESOLVED_INTERNAL: &str = "resolved_internal";
pub const RESOLUTION_EXTERNAL_REFERENCE: &str = "external_reference";
pub const RESOLUTION_UNRESOLVED: &str = "unresolved";

pub const CONFIDENCE_WEIGHT_HIGH: f64 = 1.0;
pub const CONFIDENCE_WEIGHT_MEDIUM: f64 = 0.6;
pub const CONFIDENCE_WEIGHT_LOW: f64 = 0.2;

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeConfidenceAssignment {
    pub provider: String,
    pub outcome: String,
    pub bucket: String,
    pub weight: f64,
}

pub fn canonical_confidence_bucket(label: &str) -> Option<&'static str> {
    match label.trim().to_ascii_lowercase().as_str() {
        CONFIDENCE_HIGH | "static" => Some(CONFIDENCE_HIGH),
        CONFIDENCE_MEDIUM => Some(CONFIDENCE_MEDIUM),
        CONFIDENCE_LOW | "heuristic" => Some(CONFIDENCE_LOW),
        _ => None,
    }
}

pub fn confidence_weight(bucket: &str) -> f64 {
    match canonical_confidence_bucket(bucket).unwrap_or(CONFIDENCE_LOW) {
        CONFIDENCE_HIGH => CONFIDENCE_WEIGHT_HIGH,
        CONFIDENCE_MEDIUM => CONFIDENCE_WEIGHT_MEDIUM,
        _ => CONFIDENCE_WEIGHT_LOW,
    }
}

pub fn default_bucket_for_outcome(outcome: &str) -> &'static str {
    match outcome.trim().to_ascii_lowercase().as_str() {
        RESOLUTION_RESOLVED_INTERNAL => CONFIDENCE_HIGH,
        RESOLUTION_EXTERNAL_REFERENCE => CONFIDENCE_MEDIUM,
        _ => CONFIDENCE_LOW,
    }
}

pub fn looks_external_reference(name: &str) -> bool {
    let candidate = name.trim();
    if candidate.is_empty() {
        return false;
    }
    let rust_external_root = candidate
        .split_once("::")
        .map(|(root, _)| root.trim())
        .unwrap_or("");
    let looks_likely_external_rust_root = matches!(
        rust_external_root,
        "std" | "core" | "alloc" | "proc_macro" | "test"
    );
    candidate.starts_with("external::")
        || candidate.contains('/')
        || candidate.contains('.')
        || looks_likely_external_rust_root
}

pub fn infer_resolution_outcome(to_symbol_id: Option<&str>, to_name: Option<&str>) -> &'static str {
    if to_symbol_id
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return RESOLUTION_RESOLVED_INTERNAL;
    }
    if to_name.map(looks_external_reference).unwrap_or(false) {
        return RESOLUTION_EXTERNAL_REFERENCE;
    }
    RESOLUTION_UNRESOLVED
}

pub fn normalize_provider(provider: Option<&str>, edge_type: Option<&str>) -> &'static str {
    if let Some(provider) = provider {
        let normalized = provider.trim().to_ascii_lowercase();
        if normalized == EDGE_PROVIDER_IMPORT_RESOLVER
            || normalized == EDGE_PROVIDER_CALL_RESOLVER
            || normalized == EDGE_PROVIDER_LEGACY
        {
            return match normalized.as_str() {
                EDGE_PROVIDER_IMPORT_RESOLVER => EDGE_PROVIDER_IMPORT_RESOLVER,
                EDGE_PROVIDER_CALL_RESOLVER => EDGE_PROVIDER_CALL_RESOLVER,
                _ => EDGE_PROVIDER_LEGACY,
            };
        }
    }

    match edge_type
        .map(|edge_type| edge_type.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("imports") => EDGE_PROVIDER_IMPORT_RESOLVER,
        Some("calls") => EDGE_PROVIDER_CALL_RESOLVER,
        _ => EDGE_PROVIDER_LEGACY,
    }
}

pub fn normalize_outcome(
    explicit_outcome: Option<&str>,
    to_symbol_id: Option<&str>,
    to_name: Option<&str>,
) -> Cow<'static, str> {
    if let Some(outcome) = explicit_outcome {
        let normalized = outcome.trim().to_ascii_lowercase();
        if normalized == RESOLUTION_RESOLVED_INTERNAL {
            return Cow::Borrowed(RESOLUTION_RESOLVED_INTERNAL);
        }
        if normalized == RESOLUTION_EXTERNAL_REFERENCE {
            return Cow::Borrowed(RESOLUTION_EXTERNAL_REFERENCE);
        }
        if normalized == RESOLUTION_UNRESOLVED {
            return Cow::Borrowed(RESOLUTION_UNRESOLVED);
        }
    }
    Cow::Borrowed(infer_resolution_outcome(to_symbol_id, to_name))
}

pub fn assign_edge_confidence(
    provider: Option<&str>,
    edge_type: Option<&str>,
    explicit_outcome: Option<&str>,
    to_symbol_id: Option<&str>,
    to_name: Option<&str>,
    explicit_bucket: Option<&str>,
) -> EdgeConfidenceAssignment {
    let provider = normalize_provider(provider, edge_type).to_string();
    let outcome = normalize_outcome(explicit_outcome, to_symbol_id, to_name).into_owned();
    let bucket = explicit_bucket
        .and_then(canonical_confidence_bucket)
        .unwrap_or_else(|| default_bucket_for_outcome(&outcome))
        .to_string();
    let weight = confidence_weight(&bucket);
    EdgeConfidenceAssignment {
        provider,
        outcome,
        bucket,
        weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FLOAT_TOLERANCE: f64 = 1e-9;

    #[test]
    fn canonical_confidence_bucket_supports_legacy_labels() {
        assert_eq!(canonical_confidence_bucket("high"), Some(CONFIDENCE_HIGH));
        assert_eq!(
            canonical_confidence_bucket("medium"),
            Some(CONFIDENCE_MEDIUM)
        );
        assert_eq!(canonical_confidence_bucket("low"), Some(CONFIDENCE_LOW));
        assert_eq!(canonical_confidence_bucket("static"), Some(CONFIDENCE_HIGH));
        assert_eq!(
            canonical_confidence_bucket("heuristic"),
            Some(CONFIDENCE_LOW)
        );
    }

    #[test]
    fn assign_edge_confidence_maps_generic_outcomes_deterministically() {
        let resolved = assign_edge_confidence(
            Some(EDGE_PROVIDER_IMPORT_RESOLVER),
            Some("imports"),
            Some(RESOLUTION_RESOLVED_INTERNAL),
            Some("stable::token"),
            None,
            None,
        );
        assert_eq!(resolved.bucket, CONFIDENCE_HIGH);
        assert!((resolved.weight - CONFIDENCE_WEIGHT_HIGH).abs() < FLOAT_TOLERANCE);

        let external = assign_edge_confidence(
            Some(EDGE_PROVIDER_IMPORT_RESOLVER),
            Some("imports"),
            Some(RESOLUTION_EXTERNAL_REFERENCE),
            None,
            Some("github.com/org/pkg/auth"),
            None,
        );
        assert_eq!(external.bucket, CONFIDENCE_MEDIUM);
        assert!((external.weight - CONFIDENCE_WEIGHT_MEDIUM).abs() < FLOAT_TOLERANCE);

        let unresolved = assign_edge_confidence(
            Some(EDGE_PROVIDER_CALL_RESOLVER),
            Some("calls"),
            Some(RESOLUTION_UNRESOLVED),
            None,
            Some("validate"),
            None,
        );
        assert_eq!(unresolved.bucket, CONFIDENCE_LOW);
        assert!((unresolved.weight - CONFIDENCE_WEIGHT_LOW).abs() < FLOAT_TOLERANCE);
    }

    #[test]
    fn infer_resolution_outcome_supports_mixed_language_external_names() {
        assert_eq!(
            infer_resolution_outcome(None, Some("std::fs::read_to_string")),
            RESOLUTION_EXTERNAL_REFERENCE
        );
        assert_eq!(
            infer_resolution_outcome(None, Some("pkg.module.validate_token")),
            RESOLUTION_EXTERNAL_REFERENCE
        );
        assert_eq!(
            infer_resolution_outcome(None, Some("github.com/org/pkg/auth")),
            RESOLUTION_EXTERNAL_REFERENCE
        );
        assert_eq!(
            infer_resolution_outcome(None, Some("validate_token")),
            RESOLUTION_UNRESOLVED
        );
        assert_eq!(
            infer_resolution_outcome(None, Some("auth::validate_token")),
            RESOLUTION_UNRESOLVED
        );
    }
}
