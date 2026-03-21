Great—this is the right place to start. Tokenization will quietly determine whether your entire system feels “right” or subtly broken.

Below is a **proper RFC-style spec** you can implement directly in Rust.

---

# 📄 RFC-001: Tokenization Specification

## Status

Draft → Target: Accepted

---

## 1. 🎯 Goals

Define a **deterministic, reproducible, high-performance tokenizer** for Hacker News comments that:

* Produces consistent results across time
* Preserves meaningful technical tokens (e.g. `c++`, `node.js`, `gpt-4`)
* Is fast enough for large-scale batch processing
* Avoids dependence on ML/NLP models

---

## 2. ❌ Non-Goals

This tokenizer will **not**:

* Perform sentence segmentation
* Detect named entities
* Normalize synonyms (e.g. “js” ≠ “javascript”)
* Use context-dependent rules
* Attempt grammatical correctness

---

## 3. 🧠 Design Principles

### 3.1 Determinism > correctness

Same input must always produce identical tokens.

### 3.2 Stability over time

Rules must not change frequently—changes invalidate historical comparability.

### 3.3 HN-native behavior

Prioritize:

* programming languages
* libraries
* file formats
* version strings

over traditional English NLP correctness.

---

## 4. 🔄 Processing Pipeline

Each comment goes through:

```id="pipeline"
HTML → Plain Text → Normalize → Tokenize → Emit tokens
```

---

## 5. 🧱 Step-by-Step Specification

## 5.1 HTML Stripping

### Input:

Raw HN comment HTML

### Rules:

* Remove all tags
* Decode HTML entities (`&amp;`, `&lt;`, etc.)
* Preserve visible text only

### Example:

```id="html"
"<p>Hello &amp; welcome</p>"
→ "Hello & welcome"
```

---

## 5.2 Unicode Normalization

* Normalize to **NFKC**
* Remove zero-width characters
* Normalize quotes:

  * `“ ”` → `"`
  * `‘ ’` → `'`

---

## 5.3 Case Normalization

* Default: **lowercase everything**

```id="case"
"Rust is GREAT"
→ ["rust", "is", "great"]
```

Future extension:

* optional case-sensitive corpus

---

## 5.4 Token Character Rules

### Allowed characters inside tokens:

* letters: `a–z`
* digits: `0–9`
* apostrophe: `'`
* plus: `+`
* hash: `#`
* dot: `.` (conditional, see below)
* hyphen: `-` (conditional)

---

## 5.5 Token Boundary Rules

Split on any character **not allowed above**, with exceptions below.

---

## 5.6 Special Handling Rules

### 6.1 Apostrophes (keep)

* `"don't"` → `"don't"`
* `"it's"` → `"it's"`

But:

* leading/trailing `'` removed

---

### 6.2 Dots (`.`)

Keep **only if inside token**:

```id="dots"
"node.js" → ["node.js"]
"example.com" → ["example.com"]
```

Split if:

* leading or trailing
* repeated punctuation

```id="dots2"
"hello." → ["hello"]
"...hi..." → ["hi"]
```

---

### 6.3 Plus (`+`)

Preserve:

```id="plus"
"C++" → ["c++"]
```

---

### 6.4 Hash (`#`)

Preserve:

```id="hash"
"C#" → ["c#"]
```

---

### 6.5 Hyphen (`-`)

Rule:

* keep if surrounded by alphanumeric characters

```id="hyphen"
"gpt-4" → ["gpt-4"]
"state-of-the-art" → ["state-of.the.art"] ❌ (see below)
```

Clarification:

* DO NOT preserve chained hyphen phrases as one token
* Instead split:

```id="hyphen2"
"state-of-the-art"
→ ["state", "of", "the", "art"]
```

But:

```id="hyphen3"
"gpt-4" → ["gpt-4"]
"x86-64" → ["x86-64"]
```

Heuristic:

* preserve if pattern matches:

  * `[a-z0-9]+-[0-9]+`
  * `[a-z0-9]+-[a-z0-9]+` AND length ≤ 2 segments

---

### 6.6 URLs

Strip protocol and tokenize domain:

```id="url"
"https://example.com/foo"
→ ["example.com", "foo"]
```

---

### 6.7 Numbers

Keep as tokens:

```id="numbers"
"GPT-4 is 10x better"
→ ["gpt-4", "is", "10x", "better"]
```

---

### 6.8 Remove Empty Tokens

Always discard:

* empty strings
* tokens of length 0

---

## 6. 🔢 Examples (Ground Truth)

### Example 1

```id="ex1"
"Mr. Right? I don't know..."
→ ["mr", "right", "i", "don't", "know"]
```

---

### Example 2

```id="ex2"
"C++ vs Rust vs Go"
→ ["c++", "vs", "rust", "vs", "go"]
```

---

### Example 3

```id="ex3"
"Node.js + React.js ecosystem"
→ ["node.js", "react.js", "ecosystem"]
```

---

### Example 4

```id="ex4"
"Check https://example.com/test"
→ ["check", "example.com", "test"]
```

---

### Example 5

```id="ex5"
"State-of-the-art models"
→ ["state", "of", "the", "art", "models"]
```

---

## 7. ⚡ Performance Requirements

* Must process **millions of comments per minute**
* Streaming-friendly (no full buffering required)
* Parallelizable across threads

---

## 8. 🧪 Testing Strategy

### 8.1 Golden tests

* fixed input → exact token output

### 8.2 Edge cases

* punctuation-heavy strings
* code snippets
* malformed HTML

### 8.3 Regression safety

* tokenizer version must be versioned
* any change requires full re-index or version fork

---

## 9. 🔒 Versioning

Define:

```id="version"
tokenizer_version = "v1"
```

Future changes:

* must increment version
* must not silently alter historical data

---

## 10. 🚧 Open Questions

### Q1: Case sensitivity toggle?

* v1: no (lowercase only)

### Q2: Preserve camelCase?

* likely no in v1

### Q3: Emoji handling?

* ignore for now

---

## 11. 🧠 Key Design Tradeoff

We explicitly choose:

> **Consistency over linguistic correctness**

This means:

* “Mr. Right” is not treated as a named entity
* punctuation ambiguity is ignored
* results are stable and comparable over time

---

## 12. ✅ Acceptance Criteria

* Given any input string, tokenizer produces deterministic output
* Matches all examples above
* Handles HN-style technical text correctly
* No dependency on external NLP libraries

---

## 🧭 Final Recommendation

This spec is intentionally:

* simple
* strict
* slightly “dumb”

That is **exactly what you want** for an n-gram viewer.

---

If you’re ready, next RFC should be:

👉 **RFC-002: N-gram generation + pruning strategy**

That’s where performance and storage really get decided.

