# PROFILING-GMON-CLAUSES: test the gmon profiling clauses (monstartup, mcount, __fentry__, cyg hooks)

Id: PROFILING-GMON-CLAUSES
Kind: code
Priority: 64
Size: S
Host: -
Stacks-on: PROFIL-SIGPROF-HISTOGRAM (same file, crates/core/tests/s28_profiling_clauses.rs; cut from origin/next after it lands)
Model: opus
Effort: high
BUDGET: 45 calls / 90k

## The change

Clauses (all still pending `clause!` stubs in `crates/core/tests/s28_profiling_clauses.rs`): `s28_1_c_1` (monstartup
allocates the histogram and arc tables and starts sampling; `_mcleanup` writes `gmon.out`, or
`$GMON_OUT_PREFIX.<pid>`, at exit), `s28_1_c_2` (`mcount`/`_mcount` record the call arc), `s28_3_c_0` (`__fentry__`
records the arc as `mcount` does), `s28_1_c_4` (`__cyg_profile_func_enter/_exit` are empty hooks the application can
interpose). `s28_1_c_3` (profil) belongs to PROFIL-SIGPROF-HISTOGRAM; do not touch it.

Liella code the tests reach (exists today): `monstartup`, `__monstartup`, `moncontrol`, `mcount_record`, `_mcleanup`,
`__cyg_profile_func_enter/_exit` in `crates/boundary/src/posix/s28_profiling.rs`; `Text`, `Arcs::record`, `Arcs::each`
in `crates/core/src/prof.rs`; the `mcount`, `_mcount`, `__fentry__` entry points in `csys/src/machine/mcount.S`.
Existing coverage to reuse, not duplicate: `sym_mcount`, `sym__mcount`, `sym___fentry__` and the `start`/`arcs_into`
helpers in `crates/boundary/tests/s28_profiling_symbols.rs`; `gcrt1_profiles_like_the_reference` and its
`GMON_OUT_PREFIX` runner in `crates/core/tests/gmon_products.rs`. `sym_monstartup`, `sym__mcleanup` and
`sym___cyg_profile_func_enter/_exit` there are still pending `symbol!` stubs: specify them in this story too.

Tests to write (replace each stub with a `#[test] fn` of the same name; name the clause in the body):
1. `s28_1_c_2`, `s28_3_c_0`: an arc recorded through `mcount` and through `__fentry__` (reuse the symbols-file
   helpers or call the same entry points) is present once with count N after N calls, with the right from/to pair.
2. `s28_1_c_1` + `sym_monstartup` + `sym__mcleanup`: in a child process, `monstartup(low, high)` over a known text
   range, run a profiled function, exit; `gmon.out` appears in the child's fresh directory; with `GMON_OUT_PREFIX=p`
   the file is `p.<pid>` instead and no `gmon.out` is written. Check the header and that the recorded arc is in the
   file (the format is the one `gmon_products.rs` already parses). Probe glibc for the file name rule first
   (static gcrt1 program, both settings) and record the result in the test comment.
3. `s28_1_c_4` + the two cyg symbols: calling the hooks has no observable effect (errno sentinel unchanged, no
   output); a static program that defines its own `__cyg_profile_func_enter/_exit` and is built with
   `-finstrument-functions` links against `libc.a` without a duplicate-definition error and its hooks run (count
   calls). Probe glibc with the same program.

Plants (each test run under `timeout 600`, RAM copy, fresh directory per run, restore with `cat backup > file`;
a pattern that does not apply must fail the plants script):
(a) `Arcs::record` returns without storing -> tests 1 and 2 CAUGHT; (b) `_mcleanup` ignores `GMON_OUT_PREFIX` ->
test 2 CAUGHT; (c) `__fentry__` in mcount.S skips the record call (anchor the perl pattern to the
`liella_machine_fentry` label) -> `s28_3_c_0` CAUGHT; (d) `__cyg_profile_func_enter` sets errno to EINVAL -> test 3
CAUGHT. The plants script prints CAUGHT/SURVIVED per plant and exits non-zero on a survivor.
JOBS: profiling-gmon-clauses-gate, profiling-gmon-clauses-plants, profiling-gmon-clauses-static
