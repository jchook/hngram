# RFC-010: Watermark-Based Incremental Ingestion

## Status: Draft

## Problem

The current `ingest` command tracks progress per-month (done/not-done in the manifest). This means:
- No sub-month updates — can't ingest daily
- Re-running for the same month either skips entirely or re-processes everything
- No way to pick up new comments added to an existing month's parquet file

## Goal

Support daily (or any frequency) updates. A user should be able to run `ingest` at any cadence and only new comments are processed. Pure append, no re-processing, no deletes.

## Design

### Watermark

A single `i64` value in the manifest: `last_ingested_ts` (milliseconds since epoch). This is the timestamp of the newest comment processed in the last run.

On each run:
1. Read the watermark from manifest (0 if first run)
2. For each month's parquet file, read comments and skip any with `time <= watermark`
3. Tokenize and count only the new comments
4. Update vocabulary, insert counts
5. Save the new watermark (max timestamp seen in this run)

### Ingest flow

```
ingest(data_dir, months, manifest, ch):
    watermark = manifest.last_ingested_ts  // 0 on first run
    global_counts = load_cumulative_snapshot(data_dir)
    prev_vocab = ch.load_vocabulary()
    max_ts = watermark

    for each month in months:
        // Read parquet, filter to comments with time > watermark
        comments = read_comments_after(parquet_path, watermark)
        if comments.is_empty(): continue

        // Tokenize once
        counter = process_comments_parallel(&comments)

        // Track max timestamp seen
        max_ts = max(max_ts, max timestamp in comments)

        // Update global counts incrementally
        month_globals = counter.global_counts()
        global_counts += month_globals

        // Rebuild vocabulary, delta insert new admissions
        vocabulary = build_vocabulary(&global_counts, &config)
        new_admissions = vocabulary.keys() - prev_vocab.keys()
        if new_admissions: ch.insert_vocabulary(new_rows)
        prev_vocab.merge(new_admissions)
        vocabulary.merge(prev_vocab)  // append-only

        // Filter and insert counts
        filtered = counter.filter_to_vocabulary(&vocabulary, &config)
        ch.insert_ngram_counts(filtered)
        ch.insert_bucket_totals(counter.totals())

    // Persist state
    manifest.last_ingested_ts = max_ts
    manifest.save()
    save_cumulative_snapshot(global_counts)
```

### Parquet reading with watermark filter

The `Comment` struct gains a `ts_ms: i64` field. A new `read_comments_after(path, min_ts)` function reads the parquet file and skips comments where `time <= min_ts`. It returns both the filtered comments and the max timestamp seen.

The timestamp is already available as `time_col.value(i)` (i64 millis) in the existing reader. The filter is a single `if ts_ms <= min_ts { continue; }` in the existing filter chain.

Months with no new comments (all timestamps <= watermark) return an empty vec and are naturally skipped.

### Manifest changes

Add `last_ingested_ts: i64` (default 0, `#[serde(default)]`). The existing per-month `ingest_completed` tracking becomes redundant — the watermark subsumes it.

### What stays the same

- Cumulative snapshot for global counts (already in `ingest.rs`)
- Vocabulary is append-only, loaded from ClickHouse, delta-inserted
- `--start` and `--end` args still scope which parquet files to scan
- Legacy `vocabulary` and `backfill` commands unchanged

## Operational examples

First full ingest:
```
cargo run -p ingestion -- ingest --start 2024-01 --end 2024-12
```
Watermark set to the max timestamp across all of 2024.

Daily update (re-download current month's parquet, run ingest):
```
cargo run -p ingestion -- download --start 2025-03 --end 2025-03
cargo run -p ingestion -- ingest --start 2025-03 --end 2025-03
```
Only comments newer than the watermark are processed. If no new comments, nothing happens.

Re-run is a no-op:
```
cargo run -p ingestion -- ingest --start 2024-01 --end 2024-12
```
All comments in all months are <= watermark, so nothing is processed.

## Edge cases

- **Out-of-order timestamps**: If a parquet file contains comments with timestamps older than the watermark (e.g., backdated edits), they are skipped. This is acceptable — the data set is append-only and we don't retroactively adjust.

- **Parquet file re-downloaded with corrections**: If older comments are modified (not just new ones added), those changes won't be picked up. This is consistent with the append-only design. A full rebuild handles this case.

- **Watermark corruption**: If the manifest is deleted, watermark resets to 0 and everything re-processes. Since counts tables use plain MergeTree, this would create duplicates. The user should clear ClickHouse tables before a full rebuild.

## Files to modify

| File | Change |
|------|-------|
| `server/crates/ingestion/src/manifest.rs` | Add `last_ingested_ts: i64` field |
| `server/crates/ingestion/src/parquet.rs` | Add `ts_ms` to Comment, add `read_comments_after()` |
| `server/crates/ingestion/src/ingest.rs` | Replace per-month tracking with watermark flow |
