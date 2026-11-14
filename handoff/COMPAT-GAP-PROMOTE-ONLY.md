# COMPAT-GAP-PROMOTE-ONLY: an id gap warns; it fails only under LIELLA_PROMOTE=1

- Branch task/compat-gap-promote-only. Change: xtask/src/compat_ids.rs. `check_ids(rows, refs, promote)` returns
  (errors, gaps); a gap moves into errors only when `LIELLA_PROMOTE=1`; otherwise `check` prints
  `compat-ids: warning: ... COMPAT-n is missing` for each missing id and passes.
- Unchanged and fatal everywhere: a reference to an id with no record. Duplicates are impossible by construction
  (one file per id), so no duplicate test was added.
- Tests (xtask compat_ids::): gap_warns_without_promote, gap_fails_with_promote, reference_without_record_fails.
- Plants: handoff/COMPAT-GAP-PROMOTE-ONLY-plants.txt (2). Decision: records/pending/COMPAT-GAP-PROMOTE-ONLY-1.toml.
- The promotion run must set LIELLA_PROMOTE=1 (already required by D-X.86).
- Next: nothing; read the jobs' results.
