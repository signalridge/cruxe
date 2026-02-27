/// Estimate token usage with a conservative approximation.
///
/// Formula: ceil(whitespace_split_word_count * 1.3)
pub fn estimate_tokens(text: &str) -> usize {
    let word_count = text.split_whitespace().count();
    if word_count == 0 {
        return 0;
    }
    ((word_count as f64) * 1.3).ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::estimate_tokens;

    #[test]
    fn estimate_tokens_empty_string() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("   \n\t"), 0);
    }

    #[test]
    fn estimate_tokens_single_word() {
        assert_eq!(estimate_tokens("token"), 2);
    }

    #[test]
    fn estimate_tokens_code_snippet_with_identifiers() {
        let snippet = "fn validateUserToken(token: &str) -> bool";
        assert_eq!(estimate_tokens(snippet), 7);
    }

    #[test]
    fn estimate_tokens_large_text_block() {
        let text = "word ".repeat(100);
        assert_eq!(estimate_tokens(&text), 130);
    }
}
