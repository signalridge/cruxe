pub(crate) fn normalize_relevance_score(score: f64) -> f64 {
    if !score.is_finite() || score <= 0.0 {
        return 0.0;
    }
    (score / (score + 1.0)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::normalize_relevance_score;

    #[test]
    fn normalize_relevance_score_handles_bounds() {
        assert_eq!(normalize_relevance_score(-1.0), 0.0);
        assert_eq!(normalize_relevance_score(f64::NAN), 0.0);
        assert_eq!(normalize_relevance_score(0.0), 0.0);
        assert!(normalize_relevance_score(10.0) > 0.9);
        assert!(normalize_relevance_score(f64::INFINITY) <= 1.0);
    }
}
