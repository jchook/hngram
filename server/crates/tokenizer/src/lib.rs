//! HN N-gram tokenizer library
//!
//! Deterministic tokenization for Hacker News comments.
//! See RFC-001 for specification.
//!
//! Also provides n-gram counting and aggregation per RFC-002.

pub mod counter;

pub use counter::{
    build_vocabulary, BucketKey, DenominatorContribution, NgramCounter, NgramKey, PruningConfig,
};

use once_cell::sync::Lazy;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

/// Tokenizer version - increment on any rule change
pub const TOKENIZER_VERSION: u8 = 1;

// Compiled regexes for URL detection
static URL_WITH_PROTOCOL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"https?://([a-zA-Z0-9][-a-zA-Z0-9]*\.)+[a-zA-Z]{2,}[^\s]*").unwrap());

static URL_WITHOUT_PROTOCOL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"([a-zA-Z0-9][-a-zA-Z0-9]*\.)+(com|org|net|io|dev|ai|co|app|edu|gov|me|info|biz)[^\s]*")
        .unwrap()
});

static DOMAIN_EXTRACT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:https?://)?([a-zA-Z0-9][-a-zA-Z0-9]*(?:\.[a-zA-Z0-9][-a-zA-Z0-9]*)+)").unwrap()
});

// Regex for technical hyphenated tokens that should be preserved
// Matches: gpt-4, x86-64, arm64-v8a, python-3, v8-10, es2015-2020
static TECHNICAL_HYPHEN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:[a-z]+[0-9]+-[a-z0-9]+|[a-z0-9]+-[0-9]+|[a-z]{1,4}[0-9]+-[0-9]+)$").unwrap()
});

// HTML tag pattern
static HTML_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]*>").unwrap());

/// Tokenize a comment into a list of tokens.
///
/// Pipeline: HTML → Plain Text → Normalize → Tokenize → Emit
pub fn tokenize(text: &str) -> Vec<String> {
    // Step 1: Strip HTML and decode entities
    let plain = strip_html(text);

    // Step 2: Handle URLs - replace with just domain
    let with_domains = extract_url_domains(&plain);

    // Step 3: Unicode normalization (NFKC) + quote normalization
    let normalized = normalize_unicode(&with_domains);

    // Step 4: Lowercase
    let lowercased = normalized.to_lowercase();

    // Step 5: Tokenize
    tokenize_text(&lowercased)
}

/// Strip HTML tags and decode entities
fn strip_html(text: &str) -> String {
    // Remove HTML tags
    let without_tags = HTML_TAG.replace_all(text, " ");

    // Decode HTML entities
    html_escape::decode_html_entities(&without_tags).into_owned()
}

/// Find URLs and replace them with just the domain
fn extract_url_domains(text: &str) -> String {
    let mut result = text.to_string();
    let mut offset: isize = 0;

    // Collect all URL matches first (with protocol)
    let matches: Vec<_> = URL_WITH_PROTOCOL
        .find_iter(text)
        .map(|m| (m.start(), m.end(), m.as_str().to_string()))
        .collect();

    // Replace from start to end, tracking offset changes
    for (start, end, url) in matches {
        let domain = extract_domain(&url).unwrap_or_default();
        let adj_start = (start as isize + offset) as usize;
        let adj_end = (end as isize + offset) as usize;
        let replacement = format!(" {} ", domain);
        result = format!(
            "{}{}{}",
            &result[..adj_start],
            replacement,
            &result[adj_end..]
        );
        offset += replacement.len() as isize - (end - start) as isize;
    }

    // Now handle URLs without protocol, but only if they have a path component
    // (to avoid re-matching bare domains we just extracted)
    let text2 = result.clone();
    let matches: Vec<_> = URL_WITHOUT_PROTOCOL
        .find_iter(&text2)
        .filter(|m| m.as_str().contains('/')) // Only match if has path
        .map(|m| (m.start(), m.end(), m.as_str().to_string()))
        .collect();

    offset = 0;
    for (start, end, url) in matches {
        let domain = extract_domain(&url).unwrap_or_default();
        let adj_start = (start as isize + offset) as usize;
        let adj_end = (end as isize + offset) as usize;
        let replacement = format!(" {} ", domain);
        result = format!(
            "{}{}{}",
            &result[..adj_start],
            replacement,
            &result[adj_end..]
        );
        offset += replacement.len() as isize - (end - start) as isize;
    }

    result
}

/// Extract domain from a URL
fn extract_domain(url: &str) -> Option<String> {
    DOMAIN_EXTRACT
        .captures(url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// Unicode normalization: NFKC + quote normalization + zero-width removal
fn normalize_unicode(text: &str) -> String {
    text.nfkc()
        .filter(|c| !is_zero_width(*c))
        .map(normalize_quote)
        .collect()
}

fn is_zero_width(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'  // zero-width space
        | '\u{200C}' // zero-width non-joiner
        | '\u{200D}' // zero-width joiner
        | '\u{FEFF}' // byte order mark
    )
}

fn normalize_quote(c: char) -> char {
    match c {
        '\u{201C}' | '\u{201D}' => '"',  // " "
        '\u{2018}' | '\u{2019}' => '\'', // ' '
        _ => c,
    }
}

/// Core tokenization logic
fn tokenize_text(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if is_token_char(c) {
            current.push(c);
        } else {
            // Boundary character - emit current token if any
            if !current.is_empty() {
                if let Some(token) = finalize_token(&current) {
                    // Check if this token contains hyphens and should be split
                    if token.contains('-') && !is_technical_hyphenated(&token) {
                        // Split on hyphens
                        for part in token.split('-') {
                            if let Some(t) = finalize_token(part) {
                                tokens.push(t);
                            }
                        }
                    } else {
                        tokens.push(token);
                    }
                }
                current.clear();
            }
        }
        i += 1;
    }

    // Don't forget the last token
    if !current.is_empty() {
        if let Some(token) = finalize_token(&current) {
            if token.contains('-') && !is_technical_hyphenated(&token) {
                for part in token.split('-') {
                    if let Some(t) = finalize_token(part) {
                        tokens.push(t);
                    }
                }
            } else {
                tokens.push(token);
            }
        }
    }

    tokens
}

/// Check if character is allowed inside a token
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '\'' | '+' | '#' | '.' | '-')
}

/// Clean up a token: strip leading/trailing punctuation, validate
fn finalize_token(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }

    // Strip leading punctuation (except for meaningful ones at start)
    let mut token = raw.to_string();

    // Strip leading/trailing dots
    while token.starts_with('.') {
        token.remove(0);
    }
    while token.ends_with('.') {
        token.pop();
    }

    // Strip leading/trailing apostrophes
    while token.starts_with('\'') {
        token.remove(0);
    }
    while token.ends_with('\'') {
        token.pop();
    }

    // Strip leading/trailing hyphens
    while token.starts_with('-') {
        token.remove(0);
    }
    while token.ends_with('-') {
        token.pop();
    }

    // Must have some alphanumeric content
    if token.is_empty() || !token.chars().any(|c| c.is_ascii_alphanumeric()) {
        return None;
    }

    Some(token)
}

/// Check if a hyphenated token is a technical identifier that should stay intact
fn is_technical_hyphenated(token: &str) -> bool {
    TECHNICAL_HYPHEN.is_match(token)
}

/// Generate n-grams from a list of tokens
pub fn generate_ngrams(tokens: &[String], n: usize) -> Vec<String> {
    if n == 0 || tokens.len() < n {
        return vec![];
    }

    tokens
        .windows(n)
        .map(|window| window.join(" "))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC Ground Truth Tests (Section 6)

    #[test]
    fn rfc_example_1() {
        // "Mr. Right? I don't know..." → ["mr", "right", "i", "don't", "know"]
        let result = tokenize("Mr. Right? I don't know...");
        assert_eq!(result, vec!["mr", "right", "i", "don't", "know"]);
    }

    #[test]
    fn rfc_example_2() {
        // "C++ vs Rust vs Go" → ["c++", "vs", "rust", "vs", "go"]
        let result = tokenize("C++ vs Rust vs Go");
        assert_eq!(result, vec!["c++", "vs", "rust", "vs", "go"]);
    }

    #[test]
    fn rfc_example_3() {
        // "Node.js + React.js ecosystem" → ["node.js", "react.js", "ecosystem"]
        let result = tokenize("Node.js + React.js ecosystem");
        assert_eq!(result, vec!["node.js", "react.js", "ecosystem"]);
    }

    #[test]
    fn rfc_example_4() {
        // "Check https://example.com/test" → ["check", "example.com"]
        let result = tokenize("Check https://example.com/test");
        assert_eq!(result, vec!["check", "example.com"]);
    }

    #[test]
    fn rfc_example_5() {
        // "State-of-the-art models" → ["state", "of", "the", "art", "models"]
        let result = tokenize("State-of-the-art models");
        assert_eq!(result, vec!["state", "of", "the", "art", "models"]);
    }

    // HTML handling tests

    #[test]
    fn html_stripping() {
        let result = tokenize("<p>Hello &amp; welcome</p>");
        assert_eq!(result, vec!["hello", "welcome"]);
    }

    #[test]
    fn html_entities() {
        let result = tokenize("10 &gt; 5 &lt; 20");
        assert_eq!(result, vec!["10", "5", "20"]);
    }

    // Dot handling tests

    #[test]
    fn dots_inside_token() {
        assert_eq!(tokenize("node.js"), vec!["node.js"]);
        assert_eq!(tokenize("example.com"), vec!["example.com"]);
    }

    #[test]
    fn dots_at_boundaries() {
        assert_eq!(tokenize("hello."), vec!["hello"]);
        assert_eq!(tokenize("...hi..."), vec!["hi"]);
    }

    // Plus and hash handling

    #[test]
    fn plus_preserved() {
        assert_eq!(tokenize("C++"), vec!["c++"]);
        assert_eq!(tokenize("C++ is great"), vec!["c++", "is", "great"]);
    }

    #[test]
    fn hash_preserved() {
        assert_eq!(tokenize("C#"), vec!["c#"]);
        assert_eq!(tokenize("F# programming"), vec!["f#", "programming"]);
    }

    // Hyphen handling

    #[test]
    fn technical_hyphens_preserved() {
        assert_eq!(tokenize("gpt-4"), vec!["gpt-4"]);
        assert_eq!(tokenize("x86-64"), vec!["x86-64"]);
    }

    #[test]
    fn natural_hyphens_split() {
        assert_eq!(
            tokenize("machine-learning"),
            vec!["machine", "learning"]
        );
        assert_eq!(
            tokenize("self-driving"),
            vec!["self", "driving"]
        );
    }

    // Apostrophe handling

    #[test]
    fn apostrophes_preserved() {
        assert_eq!(tokenize("don't"), vec!["don't"]);
        assert_eq!(tokenize("it's"), vec!["it's"]);
    }

    #[test]
    fn apostrophes_at_boundaries_removed() {
        assert_eq!(tokenize("'hello'"), vec!["hello"]);
    }

    // Case normalization

    #[test]
    fn case_normalized() {
        assert_eq!(tokenize("Rust is GREAT"), vec!["rust", "is", "great"]);
    }

    // Number handling

    #[test]
    fn numbers_preserved() {
        let result = tokenize("GPT-4 is 10x better");
        assert_eq!(result, vec!["gpt-4", "is", "10x", "better"]);
    }

    // URL handling

    #[test]
    fn url_domain_extraction() {
        assert_eq!(
            tokenize("Check out github.com/rust-lang/rust"),
            vec!["check", "out", "github.com"]
        );
    }

    // N-gram generation

    #[test]
    fn ngram_unigrams() {
        let tokens = vec!["hello".to_string(), "world".to_string()];
        assert_eq!(generate_ngrams(&tokens, 1), vec!["hello", "world"]);
    }

    #[test]
    fn ngram_bigrams() {
        let tokens = vec![
            "hello".to_string(),
            "world".to_string(),
            "today".to_string(),
        ];
        assert_eq!(
            generate_ngrams(&tokens, 2),
            vec!["hello world", "world today"]
        );
    }

    #[test]
    fn ngram_trigrams() {
        let tokens = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ];
        assert_eq!(generate_ngrams(&tokens, 3), vec!["a b c", "b c d"]);
    }

    #[test]
    fn ngram_empty() {
        let tokens: Vec<String> = vec![];
        assert_eq!(generate_ngrams(&tokens, 2), Vec::<String>::new());
    }

    #[test]
    fn ngram_insufficient_tokens() {
        let tokens = vec!["hello".to_string()];
        assert_eq!(generate_ngrams(&tokens, 2), Vec::<String>::new());
    }
}
