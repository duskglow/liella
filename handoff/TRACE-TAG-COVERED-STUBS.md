# TRACE-TAG-COVERED-STUBS

State: branch `task/trace-tag-covered-stubs`, code head 6cbd577a (see `git log origin/next..HEAD`; the report commit sits on top).
Done: the three pending `clause!` stubs are now `#[test] fn`s of the same name.

- `s22_5_c_1_2` (crates/core/tests/s22_threads_clauses.rs): runs `conformance(&["s22_compat_condvar"])`. The boundary
  test `sym_liella_compat_pthread_cond_threads` still runs it too (kept: symbol-coverage name; it costs one more run of the program).
- `s24_5_c_3` (crates/core/tests/s24_system_clauses.rs): re-executes itself under `unshare -r -u`, sets the NIS domain
  `nis.example` and host name `h.dns.example`, checks the `getdomainname` export and `system::domainname` both give
  `nis.example`. Skips with `SKIP s24_5_c_3: ...` where `unshare -r -u true` fails. glibc probe (same steps, mei,
  `unshare -r -u`): `set=0,0 get=0 domain=[nis.example]`, the same as Liella.
- `s0_6_c0_6_0` (crates/boundary/tests/s00_conventions_clauses.rs): `readelf --dyn-syms -W` on target/liella/libc.so.6
  (and libm.so.6 if built): `memcpy@GLIBC_2.2.5` and `memcpy@@GLIBC_2.14` both present; every name in the clause's list
  that has a libc/libm row in spec/abi/compat-plan.tsv has no `@@` version; at least 5 of them are exported.
- `compat_memcpy_2_2_5_is_overlap_safe` (crates/boundary/tests/s04_strings_symbols.rs) retagged `no spec clause` -> `s0_6_c0_6_0`.
- The `use spectest::clause;` imports that were no longer used are removed from the s24 and s00 files.

Results (mei, role s2, final code head):
- trace-tag-covered-stubs-static rc 0; clippy `--workspace --all-targets --all-features -D warnings` clean (foreground).
- trace-tag-covered-stubs-gate rc 0: `cargo xtask affected --base origin/next --run`, 4 suites, 600 passed, 0 failed,
  18 ignored; peak 1.7G, oom_kill=0.
- trace-tag-covered-stubs-plants rc 0 (script ~/work/trace-tag-covered-stubs/plants.sh, RAM copy, each test under
  timeout 600, restores with `cat`, a pattern that misses fails the script): baseline green (s24_5_c_3 not skipped);
  b (domainname copies nodename) CAUGHT by s24_5_c_3; d (forward byte copy in `liella_compat_memcpy`) CAUGHT by the
  overlap test; a (`old_cond_signal` returns Ok without waking) CAUGHT by s22_5_c_1_2 (program FAIL); c (generator
  emits `stime@@GLIBC_2.2.5`) CAUGHT, but by the `cargo xtask so` build failing (rc 1), before s0_6_c0_6_0 ran.

INFORMATIONAL (deviations from the brief, with reasons):
- `gets` has no row in compat-plan.tsv; spec/abi/versions.tsv lists it as `default` (`gets@@GLIBC_2.2.5`). The brief
  said to take the list from compat-plan.tsv, so `gets` is not checked. That makes the clause text "can no longer be
  linked against" wrong for `gets` per versions.tsv. Possible spec-text gap, recommend a spec owner check it.
- Plant c used `stime` instead of `gets`, since the generator never emits `gets`. The product build refused the planted
  default version before the test could run, so the plant is build-caught. The test's `@@` check is not proven by that plant.
- Plant d was put in the Rust translation `liella_compat_memcpy`, not in the `.S` stub: the overlap test calls the
  translation directly, so a jump change in `liella_compat_memcpy_GLIBC_2_2_5` cannot reach it. No direct test covers
  where that stub jumps. A follow-up could call `memcpy@GLIBC_2.2.5` from a C conformance program linked against libc.so.6.

Next step: none required. Optional follow-ups: (1) a `@@` plant that the build accepts (e.g. on a libm alias) to prove
s0_6_c0_6_0's version check itself; (2) a dynamic test through the `.S` memcpy@GLIBC_2.2.5 stub.
