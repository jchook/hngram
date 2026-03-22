//! HN N-gram tokenizer library
//!
//! Deterministic tokenization for Hacker News comments.
//! See RFC-001 for specification.

pub const TOKENIZER_VERSION: u8 = 1;

pub fn tokenize(_text: &str) -> Vec<String> {
    // TODO: Implement per RFC-001
    vec![]
}

pub fn generate_ngrams(_tokens: &[String], _n: usize) -> Vec<String> {
    // TODO: Implement per RFC-002
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_placeholder() {
        let result = tokenize("hello world");
        assert!(result.is_empty()); // Placeholder returns empty
    }
}
