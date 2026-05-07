# Tokenization

How HN comments are turned into tokens that get counted and charted. This is the only stage where text exists — after tokenization, the system stores nothing but per-day counts of (n-gram, count) pairs.

Status: implemented in `server/crates/tokenizer/src/lib.rs`. The behavior described here is the behavior in production — examples are direct test cases.

## Why it matters

Tokenization quietly determines whether the whole system feels right. Subtle rule changes shift which phrases get counted as the same thing, which fragments split into multiple tokens, and which n-grams cross the pruning threshold. Once data is ingested under one set of rules, retroactively changing them invalidates every chart.

Two principles drive every decision below:

1. **Determinism over linguistic correctness.** Same input must always produce identical tokens. We accept losing some natural-language nuance to avoid drift.
2. **Stability over time.** Rules don't get tweaked casually. Any change increments the tokenizer version (currently `1`), and old data stays tagged with its original version.

## Goals and non-goals

Goals:

- Produce consistent results across time and machines.
- Preserve technical tokens that HN talks about constantly: `c++`, `node.js`, `gpt-4`, `x86-64`.
- Run fast enough for batch processing the full HN corpus.
- No ML, no NLP models — the tokenizer is a few hundred lines of pure Rust.

Non-goals:

- Sentence segmentation.
- Named entity detection.
- Synonym normalization (`js` and `javascript` stay distinct).
- Context-sensitive rules. The same character sequence always tokenizes the same way.
- Grammatical correctness.

## Pipeline

```
HTML → Plain text → Normalize → Lowercase → Tokenize → Emit
```

Each stage is pure and order-dependent.

## Stages

### 1. HTML stripping

HN comments come as HTML. Tags are removed and entities are decoded.

```
"<p>Hello &amp; welcome</p>"  →  "Hello & welcome"
```

### 2. URL extraction

URLs are detected before tokenization and replaced with their domain only. Path segments would otherwise produce misleading n-grams (e.g., `foo bar` from `/foo/bar`), and people don't search for path components anyway.

```
"Check https://example.com/foo/bar"  →  ["check", "example.com"]
"Check out github.com/rust-lang/rust"  →  ["check", "out", "github.com"]
```

URLs with a protocol (`https?://...`) are matched directly. URLs without a protocol are only extracted if they include a path — bare domains like `example.com` are left for normal tokenization to pick up.

### 3. Unicode normalization

- Apply NFKC.
- Remove zero-width characters (`U+200B`, `U+200C`, `U+200D`, `U+FEFF`).
- Normalize curly quotes to straight: `“ ” → "`, `‘ ’ → '`.

### 4. Case normalization

Everything is lowercased. There is no case-sensitive corpus.

```
"Rust is GREAT"  →  ["rust", "is", "great"]
```

### 5. Tokenization

#### Allowed token characters

A token is built from any run of these characters:

```
a–z   0–9   '   +   #   .   -
```

A boundary is any other character. Whitespace, commas, parentheses, etc. all split tokens.

#### Special character rules

**Apostrophes** are kept inside a token; leading and trailing apostrophes are stripped.

```
"don't"   →  ["don't"]
"'hello'" →  ["hello"]
```

**Dots** are kept inside a token; leading, trailing, and runs are stripped.

```
"node.js"     →  ["node.js"]
"example.com" →  ["example.com"]
"hello."      →  ["hello"]
"...hi..."    →  ["hi"]
```

**Plus** is preserved.

```
"C++"  →  ["c++"]
```

**Hash** is preserved.

```
"C#"  →  ["c#"]
```

**Hyphens** are conditional. A hyphenated token is kept as one token only if it matches a "technical identifier" shape — version strings and architectures. Otherwise it splits.

```
"gpt-4"            →  ["gpt-4"]
"x86-64"           →  ["x86-64"]
"arm64-v8a"        →  ["arm64-v8a"]
"machine-learning" →  ["machine", "learning"]
"state-of-the-art" →  ["state", "of", "the", "art"]
```

The technical-hyphen heuristic, taken verbatim from the implementation:

```regex
^(?:[a-z]+[0-9]+-[a-z0-9]+|[a-z0-9]+-[0-9]+|[a-z]{1,4}[0-9]+-[0-9]+)$
```

This catches `gpt-4`, `python-3`, `x86-64`, `es2015-2020`, and similar. It misses some technical compounds (`big-data`, `front-end`) and over-splits some natural-language hyphens, but the line is consistent and stable. Edge cases will exist; consistency matters more than perfection.

**Numbers** are kept as-is, including when followed by letters.

```
"GPT-4 is 10x better"  →  ["gpt-4", "is", "10x", "better"]
```

**Empty tokens and tokens with no alphanumeric content** are discarded after boundary-stripping.

## Worked examples

These are the ground-truth tests that gate every release of the tokenizer:

```
"Mr. Right? I don't know..."        →  ["mr", "right", "i", "don't", "know"]
"C++ vs Rust vs Go"                 →  ["c++", "vs", "rust", "vs", "go"]
"Node.js + React.js ecosystem"      →  ["node.js", "react.js", "ecosystem"]
"Check https://example.com/test"    →  ["check", "example.com"]
"State-of-the-art models"           →  ["state", "of", "the", "art", "models"]
```

## How tokens become charts

Tokenization is one stage in a longer pipeline:

1. **Tokenize** each comment (this document).
2. **Count n-grams** of order 1–3 per day. (RFC-002)
3. **Prune** rare n-grams: bigrams below 20 global occurrences and trigrams below 10 are dropped at admission. The viewer is for trends, not for proving something was ever said.
4. **Aggregate** at query time from daily buckets to monthly.
5. **Normalize** to relative frequency: `count / total n-grams of the same order in that bucket`.
6. **Smooth** in the client with a centered moving average (the slider; 0 = raw).

The y-axis on every chart is that ratio in step 5. Two phrases on the same chart are directly comparable because they share denominators of the same n-gram order.

## Versioning

Defined in `server/crates/tokenizer/src/lib.rs`:

```rust
pub const TOKENIZER_VERSION: u8 = 1;
```

Stored alongside every count row in ClickHouse as `LowCardinality(String)`. Any change to the rules above must increment this version. Old data stays tagged with its original version and is never silently re-interpreted under new rules.

## Performance

The tokenizer processes the full HN corpus (40M+ comments) on a single machine in under an hour. The pipeline is streaming and parallel — there's no full-corpus buffer step.

## Implementation notes

- HTML entity decoding via the `html-escape` crate.
- NFKC via `unicode-normalization`.
- Three URL regexes: one with protocol, one without (path-required to avoid double-matching bare domains), and one to extract the domain from a matched URL.
- All examples in this document correspond to tests in `server/crates/tokenizer/src/lib.rs` and run on every CI build.
