# WL-PREP-RELEASE-PROFILE: the workload gate counts the shipped release build

Branch `task/wl-prep-release-profile` (from origin/next). Ruling #4889 (TB-FOREIGN-SAFETY-7). Decision: pending record
`records/pending/WL-PREP-RELEASE-PROFILE-1.toml` (series D-10, id to be assigned).

## What changed

* `validation/wl_lock.sh`: `wl_build_products` builds with `LIELLA_FULL_PROFILE=1` (xtask's `Profile::for_tests`
  switch, the same one the release tooling uses; no copied flags) and refuses a build whose `target/liella/.profile`
  marker is not the wanted profile; the marker is kept in the products directory. New helpers: `wl_profile`
  (`WL_PROFILE`, default `release`, `fast` for comparisons), `wl_key <sha>` (`<sha>-release`; `<sha>` for fast, the old
  key: every set cached before this change is fast, so old caches stay readable as fast and are never taken for
  release), `wl_vkey` (glibc stays glibc), `wl_products_ok` (no marker = fast). A set of the other profile under a key
  is refused, never reused, rebuilt over or deleted. Build log: `~/wl/src/<key>.build.log`.
* Consumers changed to the key (all under validation/; xtask has no `~/wl/products` reader, its `products` mentions are
  target/liella): `wl_prep.sh` (products + kit), `wl_percall.sh` (presence check, passes `WL_PROFILE` to the host
  side), `wl_percall_run.sh` (products, per-call binaries and rows `~/wl/percall/<key>`; prints `products profile:`),
  `wl_local.sh` (source copy and base presence), `wl_count.sh` (presence, passes `WL_PROFILE`), `wl_count_run.sh`
  (kits and count rows `~/wl/count/<key>`), `wl_run.sh`, `wl_attr.sh`, `wl_gate.sh` (presence, relay/rsync of kits),
  `wl_alloctrace.sh`, `wl_alloctrace_run.sh`, `wl_prime_base.sh`, `mcs_run.sh`, `validation/README.md`.
  Variant names in outputs (wl_run tsv, wl_report) stay the plain sha; only paths use the key.
* `validation/wl_lock_test.sh` (new self-test, 0.1 s, stand-in `cargo` in a scratch HOME) and its `affected` mapping
  (`VALIDATION_COVERED`, `VALIDATION_SELF_TESTS`; wl_lock.sh left `VALIDATION_UNCOVERED`).

## Results

See handoff/WL-PREP-RELEASE-PROFILE-report.md (filled at the end of the run).

## Not to redo / notes

* The first count of each commit now builds fat LTO in `~/wl/src/<sha>` (minutes longer). A primed base whose only
  cached products are the old fast set needs its source again (`wl_prime_base.sh`), as the presence checks now look
  for `<sha>-release`.
* The consumer path changes outside wl_lock.sh have no gate self-test (lab drivers need kits and hosts); they were
  checked with `bash -n` and, for wl_percall/wl_percall_run/wl_lock, by the count job of this story.

## Next step

Land; the workload gate's next baseline is release products (the first wl_gate after landing builds both commits).
