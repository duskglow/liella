# COLDSTART-TZ-FIRST: first localtime_r with TZ=America/New_York

State: branch task/coldstart-tz-first (from origin/next 60cdbdce), code commit b12ec539 (handoff commit on top).
Result: PENDING JOBS: code done and measured by counts; gate, plants and the N100 re-time are running (labels below).

## Step 0 (re-time on next)
The planning job nozomi:CSP1106-n100 (DONE) re-timed next 730f416b on the N100: first_localtime_r_new_york 46874 ns/op
(Liella only; that bench run prints no glibc side for the first_ rows). Its probe on the N100: SCIL - SCI = Liella
+37.0k user / +100.6k kernel instructions / +16 faults, glibc +11.8k / +36.2k / +4. Not at parity, so the change went ahead.

## Measured first (undiluted probe, keke, Broadwell-EP; fresh process, one localtime_r minus none, perf stat 50 runs)
- Liella +52.7k user, +356k kernel, +41 faults; glibc +13.8k, +64k, +5. The undiluted probe includes Liella's first heap
  use (8+8+16+32 MiB reservations and two munmaps); in the bench the heap is already warm (setlocale, iconv run first).
- Syscalls of the zone load: openat, fstat, read (3553 asked, 3552 got), read (EOF), close. Already one read of the body.
- perf record by symbol (instructions:u, Liella L): tzif::block 15.8%, TryVec<(i64,u8)>::push 12.0%, TryVec<i64>::push
  8.0%, heap refill 5.0%, Zone::from_tzif 4.2%, rule::parse 1.7%. Faults by symbol: heap refill/first_cache (first heap
  use), runtime start, fill_rule_years, TryVec pushes, memmove.
- The parser already skipped the v1 body (brief candidate 2 was already done).

## Change (b12ec539)
- tzif::block: times and type indices read straight from the file bytes into the transitions in one pass (no temporary
  vector, no per-element push), each index and the strict order checked as it is stored.
- Zone::from_tzif: takes `sorted` from the parser's guarantee instead of scanning again.
- Test zone::tests::wide_body_and_footer_rule (spec §23.6, RFC 8536 §3.1-§3.3): a v2 file whose v1 body disagrees, a
  transition before 1901, the instant of a transition and the one before it, the footer rule in 2040, and equal times
  rejected.

## Counts (same probe, keke)
| | user instr | kernel instr | faults |
|---|---|---|---|
| Liella before | 52.7k | 356k | 41 |
| Liella after | 30.7k | 339k | 38 |
| glibc | 13.8k | 64k | 5 |
Not verified: the bench-context (SCIL - SCI) delta after the change. It was not re-run, because the planning probe source is
not in the repo.

## Jobs (keke role ~/w/n3, logs in keke:~/work/COLDSTART-TZ-FIRST/)
- keke:tzf-check (done): clippy rc=0, static rc=0, `cargo test -p liella --release --lib -- time::` 36 passed.
- keke:tzf-stage, then keke:tzf-gate (`cargo xtask affected --tier direct --files tzif.rs zone.rs`; the role dir has no
  .git, so --files replaces --base origin/next), then keke:tzf-plants (plants.txt: read-v1-body, drop-footer-rule,
  search-off-by-one, accept-equal-times, index-shift; msg: wide_body_and_footer_rule ... FAILED).
- nozomi:tzf-n100: staged `--only cbench_opt` on the N100; compare first_localtime_r_new_york with 46874 ns (CSP1106).

## Open / next
- Remaining gap (Liella 30.7k vs glibc 13.8k user; 38 vs 5 faults undiluted). Next candidates, not tried: allocate the
  rule-year table (300 x 16 B, initialised at load: 2 pages) only when the rule is first evaluated (an instant after the
  last transition); store transitions more compactly than (i64, u8) (16 B each, 3.8 KiB for New York) or as a view on the
  file bytes; read the file into a buffer that is reused. The fault count is mostly first heap use (liella_rt heap), which
  is outside this story.
- Next step: collect tzf-gate, tzf-plants (all CAUGHT) and nozomi:tzf-n100; if the N100 row is not at or below 1.05x
  of glibc, open a follow-up for the rule-year table and the transition layout.
