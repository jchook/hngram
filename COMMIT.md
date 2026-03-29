Move TOML config parsing from tokenizer to ingestion crate

Keep tokenizer free of I/O deps (serde, toml, fs) by moving config file
parsing into the ingestion crate, which owns the CLI. Tokenizer now
exposes set_threshold() and apply_env() for callers to build PruningConfig
without knowing the source format.

Also switch TOML schema from hardcoded per-n field names (min_1gram_global)
to dynamic nested tables ([pruning.1], [pruning.2], etc.) so adding future
n-gram orders doesn't require struct changes.
