# WL-SRC-EMPTY-CACHE
Branch task/wl-src-empty-cache. Cause confirmed: the pipeline status was tar's, so a failed git archive still moved an empty tree into ~/wl/src.
Changes: wl_count.sh unpacks under pipefail, requires Cargo.toml, removes the temp tree and names the missing sha plus the prime command.
"Present" now means ~/wl/src/<sha>/Cargo.toml exists (wl_count.sh, wl_local.sh wl_local_src, wl_prime_base.sh). An empty or partial dir is removed and unpacked again.
Tests: shiki, empty dir ~/wl/src/<sha>: wl_prime_base.sh printed "primed", Cargo.toml present, second run "present".
Pipeline with an unknown sha: `git archive` fatal, rc=2, 0 files left. Unknown sha to wl_prime_base fails at rev-parse, nothing created.
Not run: wl_count end to end with an unknown sha (wl_base.sh rejects an unknown rev first). Script-only change: no plants.
Gate: static + affected rc 0, clippy rc 0 (shiki).
