<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 15: Legacy tier and binary-compatibility acceptance

* **Scope:**
  * §29: RPC/XDR (RFC 4506/5531), NIS client, STREAMS stubs, removed-syscall stubs,
    `<regexp.h>`, malloc hooks as inert variables, `sys_errlist`/`sys_siglist` arrays;
  * §12.13, the `_IO_*` vtable objects (F32);
  * every superseded **versioned contract** (C0.6, H7; Appendix C.3 lists them: 262 names
    with more than one version, 545 compatibility-only names);
  * §28, profiling (`gmon.out`);
  * §1.8, the tunables policy.
* **Ledger** (items assigned to this phase by earlier phases):
  * `glob`/`globfree` versions (owner, 2026-10-03): the current contract is the default
    symbol (`glob@@GLIBC_2.27`, `glob64`), and `glob@GLIBC_2.2.5` comes as a compat symbol. The
    only observable difference (`spec/behaviour/patterns.md`, "glob compat"): with
    `GLOB_ALTDIRFUNC`, a pattern without metacharacters is checked with `gl_stat` by the old
    version and `gl_lstat` by the new, so a dangling link is no match in the old and found in the
    new.
  * At coreutils validation, record which gnulib replacements coreutils' `configure` selects
    against Liella (owner, 2026-10-03), and compare with the host glibc's selection.
  * At coreutils validation, check that `ls`, `find`, `du`, `rm -r` and similar tools behave
    sensibly when a directory is removed during traversal, now that `readdir` reports `ENOENT`
    (D-13.2; owner, 2026-10-03).
* **Symbols:** about 330.
* **Clauses:** about 15.
* **Needs:** P14.
* **Exit:** the binary-compatibility corpus (§4.2) passes. All 3,943 names are exported with
  the manifest's versions, and `cargo xtask status` shows **0 pending**.

#### Stories (PHASE-15-PLAN, 2026-10-23)

**What exists today** (checked at `6abb25d6`, `cargo xtask status` on kanon): 691 pending in the
whole tree. Phase 15 owns about 540 of them: s29 311 (303 symbols, 8 clauses), s12 74 (`_IO_*`,
§12.13), s28 17 (profiling), s16 13 (compat-only resolver/NSS names, D-16.6), s24 11 (removed
syscalls), s10 117 (112 `__*_finite`, `__clog10*`, `_LIB_VERSION`, `__signgam`; the M6 compat
clause), s02 4 (`sys_errlist`/`sys_nerr`), s22 1 (old condvars, s22_5_c_1_2), s00 1 (C0.6). The
rest belongs elsewhere: s10's 61 `family!` stubs (track M), s27's 33 and s22's 46 `td_*`/22.10 stubs
(phase 14), s16_1_g_3 (nscd), s03's two `alloca` clauses (D-3.8) and s04's three `__*_small`
names (ERRATA E6); R15-7 settles the last two groups. `spec/abi/versions.tsv` has 845 compat rows:
libc.so.6 530 (217 superseded names, 189 of them the 2.34 libpthread/librt/libdl/libanl/libutil
merge; 287 compat-only), libm.so.6 147 (35 superseded, 112 compat-only), libnsl.so.1 127,
libc_malloc_debug.so.0 28, libresolv.so.2 11, and a few others. **272 compat versions are exported
as of 2026-10-24** (tree `86f56322`; `cargo xtask static` reports "compat-gen: ok ... 272 compat
versions"): the generated `csys/src/machine/compat_aliases.S` (`cargo xtask compat-gen`) and
`stime@GLIBC_2.2.5` (D-27.4, picked up by `xtask/src/so.rs`). `cargo xtask symcov` then finds 390
compat-only names still missing (libc.so.6 232, libm.so.6 147, libresolv.so.2 11), plus libnsl.so.1
127 (see docs/spec-symbol-gaps.md). The other counts in this paragraph are those of 2026-10-23 (`6abb25d6`). `crates/legacy` holds 22k
lines of H3/H4/H8 logic; `sunrpc.rs` and `versioned.rs` are 3-line placeholders. GLIBC_TUNABLES is
already parsed (`crates/core/src/config.rs`); §1.8 still needs its policy text and tests. The
binary-compatibility corpus (§4.2) does not exist yet. Probes already in the tree: `glob@GLIBC_2.2.5`
and `isastream` (`crates/core/tests/oracle/patterns_diff.c`, `specman_probe.c`). MATH-M6-PLAN
measured glibc's `expf`/`logf`/`powf@GLIBC_2.2.5` at +2.0/+0.6/+3.5 ns over the 2.27 entries and
recommends aliasing them only if a probe shows the same behaviour (folded into LG-6).

**What needs phase 14.** Liella's `libc.so.6` cannot run a dynamic program until DL-14, and a
static link can never bind a non-default version. So before DL-14 a story proves its compat
entries at three levels: (1) link level: readelf/`xtask so` shows `sym@VER` with the manifest's type,
binding and size; (2) logic: host-side unit tests of the `liella-legacy` code; (3) behaviour:
black-box probes of glibc (C programs that bind the old version with `.symver`, run on the host
glibc) recorded in `spec/behaviour/compat.md` + `.glibc.txt`. The same probes run against Liella
come later, in LG-22. Stories marked **NOW** can start before phase 14 lands; **P14** names the
DL story they wait for.

**Proof common to all stories.** `cargo xtask static`, then `cargo xtask so` with its version check
(every new `sym@VER` in the right node, nothing exported that the manifest lacks). Plants with `msg:`
fields on the new logic and one "drop an alias" plant per generator story. Code that reads network or
file input (XDR, RPC, NIS, `/etc/rpc`) is **HIGH**: malformed-input cases in its own tests, a fuzz
target where noted, glibc weaknesses only in SECURITY-PRIVATE. INVIOLABLE: none of this may touch a
hot path. A compat alias costs nothing at run time; a story that changes shared code (stdio for
LG-8, regex for LG-10) runs the affected workload rows.

Probes, tables and docs (no library code):

1. **LG-0a `LEGACY-COMPAT-TABLE`** (S, NOW). `spec/abi/compat-plan.tsv`: one row for each of the 845
   compat rows (library, name, version, kind = `alias` (same contract as the default) /
   `behaviour` (old contract differs) / `stub` (ENOSYS or fixed result) / `object` (data with a
   manifest size) / `subsystem` (RPC, NIS, profiling), the owning story, the D-row). An xtask check
   that the two tables match row for row. Proof: the check, plus a plant that deletes one plan row
   and must fail. This is the dispatch table for LG-1 to LG-21.
2. **LG-0b `LEGACY-PROBE-COMPAT`** (M, NOW). Black-box probes of every `behaviour` row: `memcpy@2.2.5`
   with overlapping buffers, `posix_spawn{,p}@2.2.5`, `clock_*@2.2.5`, `fmemopen@2.2.5`,
   `quick_exit@2.10`, `realpath@2.2.5`, `nftw{,64}@2.2.5`, `regexec@2.2.5`, the
   `sched_/pthread_*affinity@2.3.3` size rules, `pthread_getattr_np`/`pthread_sigmask@2.2.5`, the 35
   superseded libm names (`errno`, flags, `matherr`, `_LIB_VERSION`). Proof:
   `spec/behaviour/compat.md` + `.glibc.txt`, each line from a probe in `tools/probe/`; a
   static check (`cargo xtask static`) on the probe list.
3. **LG-0c `BINCOMPAT-SWEEP-STATIC`** (S, NOW). The real-binary sweep: every ELF under the hosts'
   `/usr/bin`, `/usr/sbin`, `/usr/lib*` (verneed lists, `readelf -V`, files only) against
   `versions.tsv` and against Liella's current `libc.so.6`/`libm.so.6` exports. Output: the compat
   versions real binaries need, counted, which ranks the stories below, and the list of programs
   whose every need Liella already exports (LG-22's first corpus). An xtask subcommand or
   `validation/` script that a later story reruns. Proof: the report, reproduced on two hosts.
4. **LG-0d `LEGACY-TUNABLES-POLICY`** (S, NOW, R15-6). §1.8: which `GLIBC_TUNABLES` entries Liella
   honours, ignores or refuses, and the secure-mode rule (a cross-reference to phase 14's threat
   model); clause tests over `config.rs`. Proof: tests + a plant on the secure-mode filter.
5. **LG-0e `LEDGER-COREUTILS`** (S, NOW). The two coreutils ledger items above: the gnulib
   replacements coreutils' `configure` selects against Liella and against the host glibc (configure
   output and `config.h` only), and `ls`/`find`/`du`/`rm -r` with a directory removed during the walk
   (D-13.2). Proof: `validation/results/` notes; a differences row for every replacement that differs.

Versioned entry points:

6. **LG-1 `COMPAT-SYMVER-GEN`** (M, NOW, R15-1, R15-2). Generate the `alias` rows of `compat-plan.tsv`
   into `.symver` directives (a generated `csys/src/machine/compat_aliases.S`, directives only, K2),
   replacing the hand-written `compat_stime.S`; the 189 2.34-merge names and the
   `clock_*@2.2.5` family land here. Compat-only names stay out of `libc.a` (R15-2). Proof: `xtask so`
   version check; readelf shows every `alias` row; plant "drop one alias" fails the check.
7. **LG-2 `COMPAT-BEHAVIOUR`** (M, after LG-0b and LG-1). `liella-legacy::versioned`: the old
   contracts of `memcpy@2.2.5` (overlap-safe), `glob@2.2.5` (ledger: `gl_stat` for a
   metacharacter-free pattern under `GLOB_ALTDIRFUNC`), `fmemopen@2.2.5`, `realpath@2.2.5`,
   `nftw@2.2.5`, `regexec@2.2.5`, `quick_exit@2.10`, `posix_spawn@2.2.5`, the affinity pairs, each
   exported by LG-1's generator. Proof: host-side unit tests against LG-0b's lines; a plant per entry.
   Risk of overrun: split point after `glob`/`fmemopen`.
8. **LG-3 `COMPAT-CONDVAR-OLD`** (M, after LG-0b, HIGH). `pthread_cond_*@GLIBC_2.2.5` (s22_5_c_1_2):
   the old-ABI object layout as the clause and LG-0b's probes define it, mapped onto Liella's
   condvar. Proof: in-process stress in RAM (signal/broadcast/timedwait across threads); a plant
   that loses a wakeup must fail.
9. **LG-4 `COMPAT-LIBM`** (S, after LG-0b and LG-1). The 112 `__*_finite` (aliases of the current
   functions, D-10.2), `__clog10*`, `__signgam`, `_LIB_VERSION`, and the 35 superseded libm names:
   an alias where LG-0b shows identical behaviour (then faster than glibc's, MATH-M6-PLAN), a
   `behaviour` entry where it does not. Proof: `xtask so` for libm; s10 compat clause tests;
   plant on one alias target.
10. **LG-5 `LEGACY-STUBS`** (S, after LG-1). Removed syscalls (`bdflush`, `create_module`,
    `get_kernel_syms`, `nfsservctl`, `query_module`, `sstk`, `uselib`, `sysctl`/`__sysctl`:
    s24_6_c_6, s29_4_c_3) and the STREAMS names (`isastream`, `fattach`, `fdetach`, `getmsg`,
    `putmsg`, `getpmsg`, `putpmsg`: s29_3_c_0). Proof: clause tests (errno and result as the
    probes); `xtask so`.
11. **LG-6 `LEGACY-OBJECTS`** (S, after LG-1, HIGH). Data objects with a manifest size:
    `sys_errlist`/`_sys_errlist`/`sys_nerr`/`_sys_nerr` (s02), `sys_siglist`, the inert malloc hook
    variables, `_null_auth`, `__libc_sa_len`, the `__*_version_placeholder` names. A copy relocation
    copies the manifest's size: a wrong size is memory corruption in an old binary. Proof: an xtask
    check that every `object` row's `st_size` equals `versions.tsv`; contents equal the probe;
    plant shrinks one array.
12. **LG-7 `COMPAT-NSS-RESOLV`** (S, after LG-1). s16's 13 compat-only names (D-16.6) and libresolv's
    11 compat rows. Proof: clause tests; `xtask so`.
13. **LG-8 `IO-VTABLES`** (M, NOW, R15-3, HIGH). §12.13/F32: the 74 `_IO_*` names (default versions,
    so they link statically and test now): helper functions over Liella's `FILE`;
    `_IO_file_jumps`/`_IO_wfile_jumps` as read-only objects of the manifest size. Proof: clause
    tests; size check from LG-6; stdio workload rows unchanged; a plant per helper family.
14. **LG-9 `PROFILING-GMON`** (M, NOW). §28: `monstartup`, `_mcleanup`, `moncontrol`, `profil`,
    `sprofil`, `mcount`/`_mcount` (an `.S` that only saves registers and calls Rust, K2),
    `__profile_frequency`, `__cyg_profile_func_*`, `__fentry__`. Proof: a static `-pg` program
    writes `gmon.out`; the host `gprof` shows the same call graph as for the host glibc's build;
    `profil` histogram non-empty. Risk: `-pg -static` needs `gcrt1.o`: check in the first calls,
    and split if Liella does not ship one.
15. **LG-10 `REGEXP-H`** (S, after LG-1). `<regexp.h>` `step`/`advance`/`loc1`/`loc2`/`locs` over the
    regex engine. Proof: LG-0b-style probe lines; host-side tests.
16. **LG-11 `MALLOC-COMPAT`** (S, after LG-1; libc_malloc_debug part after DL-1). `__libc_freeres`
    (s29_4_c_1), `__libc_malloc` and friends (s29_4_c_2), libc_malloc_debug's 28 compat rows
    beyond DL-1's `malloc_get_state`/`malloc_set_state`. Proof: clause tests; `xtask so` for both
    libraries.

ONC RPC, XDR and NIS (§29.1, §29.2; all HIGH, all in `liella-legacy`):

17. **LG-12 `XDR-PRIMITIVES`** (M, NOW). RFC 4506 primitives and `xdrmem`: integers, hyper, enum,
    bool, float/double, opaque, string, bytes, `xdr_free`. Pure logic. Proof: RFC vectors and
    byte-exact probe lines from the host glibc; an `xdr` fuzz target (decode never reads out of
    bounds); plants on length checks.
18. **LG-13 `XDR-STREAMS`** (M, after LG-12). `xdrrec` (record marking), `xdrstdio`, arrays,
    vectors, unions, `xdr_reference`/`xdr_pointer`, `xdr_sizeof`, the remaining xdr_* names
    (100 in all). Proof: as LG-12; record-fragment corpus.
19. **LG-14 `RPC-SERVER`** (M, after LG-13). `svc_*` (UDP, TCP, Unix), `registerrpc`, `svc_run`,
    `xprt_*`, `svcerr_*`, `_seterr_reply`, `_authenticate`. Proof: interop with a client built on the
    host glibc (black box); malformed-call corpus; fuzz on call decode.
20. **LG-15 `RPC-CLIENT`** (M, after LG-14). `clnt_*`, `callrpc`, `pmap_*`, `rtime`,
    `bindresvport`, `_rpc_dtablesize`. Proof: Liella client against Liella server and against the
    host-glibc server; timeouts and truncated replies.
21. **LG-16 `RPC-AUTH-NETNAME`** (M, after LG-15, R15-4). `auth_*`/`authunix_*`, `authdes_*`,
    `key_*`, `ecb_crypt`/`cbc_crypt`/`xencrypt`/`xdecrypt`/`passwd2des`, `netname*`/`*2netname`,
    `getrpcent`/`byname`/`bynumber` (`/etc/rpc` parsing). Proof: probe lines (no keyserv running ⇒
    the same failures); FIPS-46 DES vectors; `/etc/rpc` malformed-line corpus.
22. **LG-17 `NIS-YP`** (M, after LG-15 and DL-1, R15-5). `libnsl.so.1` as a product (DL-1's
    placeholder generator), `yp_*`/`__yp*`. Proof: the no-domain and no-server paths equal the
    probes; `yp_match` against a test `ypserv` built on LG-14.
23. **LG-18 `NIS-PLUS`** (M, after LG-17, R15-5). The 72 `nis_*`/`__nis*` names and
    `readColdStartFile`/`writeColdStartFile`: name/string helpers fully, server operations as the
    ruling says. Proof: probes with no cold-start file; `xtask so` for libnsl.

Acceptance:

24. **LG-19 `BINCOMPAT-CORPUS`** (M, P14: after DL-21, LG-1 to LG-11). §4.2: the corpus (R15-8):
    LG-0c's program list, test programs that bind each `alias`/`behaviour`/`object` row with
    `.symver`, and LG-0b's probes rerun on Liella; run under our `ld.so` in a user + mount namespace.
    Proof: every corpus program's output equals the host glibc's; the list in `validation/`.
25. **LG-20 `PHASE15-EXIT`** (S, after all and the phase-14 exit). `cargo xtask status` at 0 pending
    (with R15-7's conversions), all 3,943 names with the manifest's versions (`xtask so` full check),
    symcov, the differences rows, README and the Delivered entry.

**Order and parallel lanes.** Now, in parallel: LG-0a, LG-0b, LG-0c, LG-0d, LG-0e, LG-8, LG-9, LG-12.
Lane *versions*: LG-0a → LG-1 → (LG-4 ‖ LG-5 ‖ LG-6 ‖ LG-7 ‖ LG-10 ‖ LG-11), with LG-2 and LG-3 after
LG-0b. Lane *RPC*: LG-12 → 13 → 14 → 15 → 16, and → 17 → 18 once DL-1 lands. Lanes *stdio* (LG-8) and
*profiling* (LG-9) stand alone. Everything except LG-17's library, the libc_malloc_debug part of
LG-11, LG-19 and LG-20 can finish before phase 14 lands; their dynamic runs come together in LG-19.
Critical path: LG-12 → 13 → 14 → 15 → 17 → 18 (6 runs), then LG-19 → LG-20 behind DL-21. 25 stories,
about 25 runs of 50 calls.

**RULING NEEDED** (rulings never block, owner 2026-10-23; "provisional" = cheap to proceed on the
recommendation and rewrite if ruled otherwise):

* **R15-1 How compat versions are made.** Recommend one generated `.S` of `.symver` directives
  from `compat-plan.tsv` (the `stime` pattern, K2-clean, no logic), and `behaviour` entries as
  Rust functions under `liella_compat_*` names that the directives alias. Provisional.
* **R15-2 Compat names in `libc.a`.** A static link cannot bind a non-default version. Recommend
  keeping compat-only names out of `libc.a` and testing their logic in-process. Provisional.
* **R15-3 `_IO_*` and the jump tables.** Recommend working helpers over Liella's `FILE` and the
  jump tables as read-only data of the manifest size that Liella's own stdio never dispatches
  through (gain: a vtable overwrite redirects nothing; differences row). Provisional.
* **R15-4 DES and keyserv.** Recommend implementing `ecb_crypt`/`cbc_crypt` to FIPS 46 (AUTH_DES
  interop needs it) with the docs marking DES as broken, and `key_*` over keyserv's socket, failing
  as glibc does when none runs. Provisional for the failure paths; the owner decides whether
  AUTH_DES is worth the full work.
* **R15-5 NIS and NIS+ depth.** Recommend real `yp_*` (still deployed) and, for NIS+ (dead),
  string helpers in full with server operations failing as glibc does with no cold-start file
  (probe-settled). Provisional.
* **R15-6 Tunables policy (§1.8).** Recommend: parse the full `GLIBC_TUNABLES` syntax, honour the
  entries that map to a Liella setting, ignore unknown ones silently as glibc does, and apply the
  phase-14 secure-mode rule. Provisional.
* **R15-7 Pending stubs that no library code can close** (s03 `alloca` D-3.8, s04 `__*_small`
  ERRATA E6). Recommend turning the `alloca` clauses into `commentary!` with the D-row, and probing
  `__*_small` black-box (readelf type/size, calls); if the probe cannot settle the parameters,
  export ENOSYS-style stubs with a differences row. Provisional.
* **R15-8 Corpus sources.** Recommend host binaries plus test-built `.symver` programs; old distro
  packages only by owner approval, fetched into an isolated directory and run only inside a user
  namespace. Not provisional for downloads; the rest is.
