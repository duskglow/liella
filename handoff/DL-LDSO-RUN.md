# DL-LDSO-RUN (phase 14 DL-6): ld.so reads auxv/env, decides secure mode, enters a dependency-free PIE

**State:** branch `task/dl-ldso-run-2` (DL-LDSO-RUN-2, continuing `task/dl-ldso-run`; merge base with origin/next
1b3a3d58e, which holds DL-5). Commits: 8973010d (change), 1662e1c2 (static fixes), 3f8b43c9 (handoff), 508fcc7d
(the two survivor tests, DL-LDSO-RUN-2), then this handoff. Role mei:~/w/s3.
Decisions in force: D-27.14 (plan/07-decisions/s26-29.md), D-27.13 (DL-5), owner #2661 (secure = AT_SECURE
only; one UNSECURE list, two call sites), R14-2 (a), R14-8 (program entry primitive), R14-18.

## Done (DL-LDSO-RUN)
* `liella_ld::start` (pure): `secure`, `program` (refuses no/zero AT_PHDR or AT_ENTRY, AT_PHENT != 56, AT_PHNUM 0
  or >= PN_XNUM), `rewrite` (`ld.so PROG`), `ld_env`, `preload_names`, `needs_objects`; `liella_ld::map` for a PIE.
* `Arch::features(&Auxv, &impl Machine)`, the only CPU-feature source. `boundary/src/ld.rs`: auxv copied (<= 128
  pairs), secure decided once, UNSECURE removed and the vector moved down, THEN the one `LD_*` read; tunables with
  the secure filter; PT_INTERP start or `ld.so PROG ARGS`; DT_NEEDED refused; RELATIVE/RELR; RELRO;
  `liella_machine_ld_enter` (entry.S). Failures: one line on fd 2, status 127.
* Tests `crates/ld/tests/ldso_run.rs` (7): malformed auxv, rewrite/env, features seam, 100 starts each way (half
  `setarch -R`), refusals, LD settings, set-group-ID secure child (no root).

## Done (DL-LDSO-RUN-2)
* Cause of the red counts job: its command used `--no-archive`, which `xtask bench` does not have. Rerun as
  `cargo xtask bench --compare --counters cbench_startup`, base and branch in the same role on mei, one after the
  other (base = a detached worktree of the merge base 1b3a3d58e synced into s3, then the branch synced back).
* Plant survivor `auxv-not-moved` (DL-6): with 34 names removed, the stale environment words between the new
  terminator and the old vector paired up, so the program's walk still reached the real vector. The test
  program now prints `auxv:ok` only if every type it finds is below 64, and every start checks it. CAUGHT.
* DL-5 survivor `link-reloc-symbols` closed: new `ldso_link_relocation_symbol_past_hash_count` links an object
  that hashes no symbol (GNU hash only), sets `symoffset` to 1 so the undefined symbol its relocation names lies
  past the count; the check must refuse it or report it (the original refuses with SymbolIndex; the mutant
  passed it unseen). CAUGHT. Linker output itself never has this shape (unhashed symbols sit below symoffset),
  so the test edits one header word.

## Results (mei)
* Previous run: `dl-ldso-run-gate` rc 0 (escalated to FULL: 11163 passed, 0 failed); `dl-ldso-run-plants`
  12 CAUGHT, 1 SURVIVED (auxv-not-moved, fixed above); `dl-ldso-run-counts` rc 1 (bad option, above).
* `dl-ldso-run-2-tests` rc 0 (ldso_run 7/7, ldso_link 9/9, no skips). `dl-ldso-run-2-plants`
  (handoff/DL-LDSO-RUN-2-plants.txt): 2 CAUGHT, 0 SURVIVED, no WARN.
* Startup counters (instructions/op, Liella; base `dl-ldso-run-2-counts-base` -> branch `dl-ldso-run-2-counts`):
  startup_instr_c 2157 -> 2157; setloc 68064 -> 68078 (+14); printf 71865 -> 71846 (-19); wctype 69125 -> 69128
  (+3); kinstr_c 186718 -> 183137 (kernel side). glibc between the two runs: +-1 on the user rows. No ld.so
  symbol is in libc.so.6 or libc.a; libc's start code (crates/rt) is unchanged on this branch. A no-build
  repeat on the same branch binaries (`dl-ldso-run-2-counts-repeat`): c 2157, setloc 68066, printf 71846,
  wctype 69128, so setloc alone varies by 12 between two runs of identical binaries. Verdict: startup does not
  move beyond the counters' run-to-run spread (parity by instructions); printf is 19 lower in both runs.
* Gate `dl-ldso-run-2-gate` (affected --run --base origin/next, escalated to FULL as before: entry.S, xtask and
  ld-so paths): stubs and workspace clippy ok on this head; `cargo xtask static` ok (foreground). The test
  phase was at 763/4997 at 09:18 UTC with mei at load ~60, so it was still running at hand-back (PENDING).
  Re-attach: `~/bin/jobwait dl-ldso-run-2-gate`; log `~/logs/dl-ldso-run-2-gate.log`.

## Arch seam checklist: ticked (unchanged from DL-LDSO-RUN)
1 literals only in arch.rs/arch/mod.rs/entry.S/x86_64-only tests; 2 DL-4 relocator; 3 n/a; 4 `liella_machine_ld_enter`;
5 features only from `Arch::features` (test pins it); 6 n/a; 7 tests cfg x86_64 with reason; 8 table; 9 nothing new
before relocation.

## Open / next
* RULING NEEDED: none new (DL-5's unsafe-audit list ruling still open).
* DL-5's `ld-skip-relocation` WARN-NO-MSG (handoff/DL-LDSO-PRODUCT.md) is not touched here.
* Gaps for DL-7: ET_EXEC programs, AT_PAGESZ (4096 assumed), the mapper's corpus; env values over 4096 bytes read
  truncated by `c_string` (only the LD_PRELOAD diagnostic uses them).
* Do not redo: the counts command above is the working one; `bench --stage --base` needs a git tree (role is not).
* Next: DL-7 (general mapper, DT_NEEDED loading).
