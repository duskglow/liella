# CONF-DYN-HELPERS (DL-16H): a dynamic mode for the conformance harness

**State.** Branch `task/conf-dyn-helpers`, **based on `origin/task/dl-libc-start-10`** (fast-forward from
next 5e5ec2a57): on next a dynamically linked program cannot start under Liella's ld.so (job
`conf-dyn-helpers-so1`, mei: "libc.so.6: ... the library search is not available in this build"; next also lacks the
dynamic `__libc_start_main`). My commits sit on top of eb99c12df and are separate from DL-LIBC-START's. Role mei:~/w/s1.
Status: INCOMPLETE (context checkpoint). Head: see handoff/reports/CONF-DYN-HELPERS.json.

**Done (two WIP commits).**
- `conformance/run.sh`: dynamic-link mode builds two helper sets and two programs: the reference's (host cc, host
  libc) and Liella's (`-nodefaultlibs` against a copy of `target/liella/libc.so.6` without DT_SONAME through a
  `libc.so` script with the host's `libc_nonshared.a`, interpreter `target/liella/ld-linux-x86-64.so.2`; the same
  method as `conformance_dynamic` in crates/boundary/tests/products/mod.rs, because ld.so has no search on the run
  path yet). Linked helpers go in by path. Both runs are judged by the EXPECT-* directives. New directives:
  `DLOPEN-HELPERS`, `HELPER-FLAGS: <helper> <flags>` (run in conformance/c/helpers, `-L` at the helper dir),
  `LINK-EXPORT-DYNAMIC`, `PENDING-LIELLA: <story>` (reference must pass; Liella failing = `PENDING` line, counted;
  Liella passing = FAIL; any build failure = FAIL; pending programs link with `--unresolved-symbols=ignore-all`).
  Programs get `CONF_HELPERS` (+ `LD_LIBRARY_PATH`) = their helper dir. Each helper's .so is removed before the build
  and checked to exist after it, in both sets (the plant target). Summary line adds `(N of them pending on Liella)`.
- Programs: `s27_dlsym_handles` (DLSYM-SPECIAL-HANDLES probe + dlsym/dlvsym failure texts; pending DL-16b),
  `s27_dlopen_failures` (run.sh errors corpus: 13 file cases made at run time, missing dep / undefined symbol /
  version via helpers with soname and version-script tricks, mode 0, undefined bit 0x10, NOLOAD, P-6 clearing;
  pending DL-16a), `s27_dynlink_helper_dlsym` now pending DL-16b. Helpers dlh_handle1/2, dlf_* + dlf_v1/v2.map.
  Left out on purpose: LAZY with an undefined function (DL-20's interim), dlclose, dlinfo (DL-17/18).
- `xtask/src/affected.rs`: any file under conformance/c/helpers/ selects every conformance program (map files).
- conformance/README.md rewritten for the mode.

**Verified.** `conf-dyn-helpers-try3` (mei, head 56fe768db's code): the three programs pass on the reference with every
probed text, and are PENDING on Liella (rc 139 / 127 as expected); s01_abort (static) still passes. 4 passed 0 failed.

**Next step (about 30-40 calls).**
1. Clause tests in `crates/core/tests/s27_dynlink_clauses.rs` (still the generated stub file: replace the header line
   by `// Specified (phase 14).` as other specified files do): `s27_1_f_0` runs `s27_dlopen_failures`, `s27_1_c_3`
   runs `s27_dlsym_handles`, each through conformance/run.sh and asserting the output holds
   `conformance: PENDING <name> on Liella until DL-16a|DL-16b` (named, counted; run.sh fails the day Liella passes).
   Doc comments name the clause and the pending story.
2. Plants file `handoff/CONF-DYN-HELPERS-plants.txt` (docs/plants.md format), at least: skip `build_helper` for the
   Liella set (`build_helper "$h" "$ldir"` -> `true`), skip it for the reference set, drop the existence check is
   EQUIVALENT only if the build checks still catch it (test it), drop `pend` FAIL branch (Liella passing not caught:
   needs a fake pass, mark equivalent with reason or plant `[ "$ok" -eq 1 ]` -> `true`). cmd: `cargo xtask affected
   --only conformance:s27_dlopen_failures --with-exports --run` or `-p liella-core --test s27_dynlink_clauses`; msg
   `helper .* (was not built|does not build)`.
3. Pending decision record: `cargo xtask records new decision D-NEW --series D-27 --story CONF-DYN-HELPERS` (harness
   design: two builds, by-path libc until the run-path search lands, PENDING-LIELLA semantics).
4. clippy, static, direct gate `cargo xtask affected --run --base origin/next` (the base is dl-libc-start-10: the
   gate will also select its files; pass `--base origin/task/dl-libc-start-10` if the coordinator prefers, say so),
   plants job; handback-check.

**Do not redo.** The base decision and its probe; the reference texts (all match).

**Open questions.** When ld.so's run path gains the library search, drop the by-path libc copy and link Liella's
builds against libc.so.6 by soname (a dlopen'd helper's DT_NEEDED `libc.so.6` must then match the loaded libc).
