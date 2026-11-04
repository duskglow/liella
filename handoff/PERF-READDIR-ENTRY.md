# PERF-READDIR-ENTRY (branch task/perf-readdir-entry-4, continued from -3, -2 and task/perf-readdir-entry)

State: code commit 8dee0d52 on top of cdf9880a (lock_alone swap, record-walk tests) and 232388b7. Stacked on
task/perf-fswalk-phys, which is not in next yet. Run 4 (PERF-READDIR-ENTRY-4, mei ~/w/s1): bound commit 54be5237,
ruling #2619 carried out (below). Run 3 (keke ~/w/n3): no code change. Runs 2 and 1 used keke ~/w/n4.

## Run 4: ruling #2619 carried out on mei (the CI host, Skylake-SP)

Job perf-readdir-entry-4-hotcount (+ -hotcount-merged): four trees from `git archive` (the merged one from
`git merge-tree --write-tree origin/next HEAD`; no branch was merged), extracted at equal-length paths under
/mnt/zram/claude/PERF-READDIR-ENTRY-4/count-{1,2}/<4 chars>, `cargo xtask so`, then the hot_text_products test 5 times
each. Every run is identical. tool / locale / startup: origin/next c3424c99 67 / 65 / 18; merge base b1f1f9f3
61 / 58 / 18; branch 7bde29e3 62 / 59 / 18; branch on top of next (tree a0e5c2e6) 68 / 66 / 19 (CI's role path: 69).
Next already sits one under the bound, and the branch adds one fault on both bases. So the delta is layout-only:
TOOL_BOUND 68 -> 71 (HOTTEXT-VARIANCE: highest count 69 plus 2), with the counts recorded in the test's doc comment. Regen
NOT committed. The `.S` layout fix (run 3's RULING (a)) goes to HOTTEXT-ASM-ORDER, not here.
Logs: mei ~/logs/perf-readdir-entry-4-hotcount*.log, per-run logs ~/work/perf-readdir-entry-4/run-<tree>-<n>.log.
Jobs on 54be5237 (mei): perf-readdir-entry-4-static (clippy --all-features -D warnings + xtask static),
perf-readdir-entry-4-plants (run 2's 3 readdir plants + tool-bound-low, fresh zram copy plants-1),
perf-readdir-entry-4-gate2 (direct tier). Superseded: perf-readdir-entry-4-gate (rc 0, on 7bde29e3, before the bound).

## Run 3: CI red on hot_text_products (tool 69 > 68 on CI's host), diagnosis only

What CI saw: init_coreutils_shaped_program_stays_within_its_page_fault_bound read 69 minor faults vs bound 68 (CI host,
f3af63a0, steady). Keke (Zen 1 host) on the same head reads tool 61, locale 58, startup 18, strings 0: all pass,
so keke does not reproduce the red. Its margin is 7 faults, CI's host is over by 1. The two hosts run different machine
paths, so counts differ by host.
Regen tried as the coordinator asked (job perf-readdir-entry-3-regen; script, logs and list.diff in keke
~/work/perf-readdir-entry-3/): `tools/hottext/regen.sh ~/w/n3 "<page_faults built like the test> tool"` (fast
profile only). It adds startup keys (rt start::program, liella_link_rela_iplt_start/end, liella_sys_mprotect) and the
tool's gettext keys (intl record/translate/textdomain, catalog lang, heap take_held, lctime Time::string,
Locale::compare_bytes) and moves a few keys (getrusage and _IO_fflush to the end, Locale::with later). Keke after:
tool 61 -> 58, locale 58 -> 56, startup 18 -> 17, but **init_first_string_calls_after_printf_take_no_text_fault goes
0 -> 1 (red)**. That job's rc is 0 only because my script did not pass on the test's rc: the job proves nothing.
Cause (job perf-readdir-entry-3-layout plus the scratch probe strprobe.c, per-call minflt deltas, setarch -R): the
hot group starts 64 KiB-aligned (0x410240) and runs to about 0x481000. The `.S` machine kernels (strcpy, memchr,
memcmp, strncmp, ...) come AFTER all the hot Rust functions, wherever their keys are in the list. Original list:
strncmp at 0x47ef00 (window 71, same window as the other string kernels). Regen list: the Rust part is 0x1980 bytes
bigger and strncmp is at 0x480880, alone at the start of window 72, so its first call faults. The readdir code is not
in the hot group (no readdir/dirtree key in the list), so on keke the readdir change does not move the group.
I did not commit the regen: it turns a passing direct test red, and I cannot check CI's host from here.

RULING NEEDED: (a) recommended: a separate story to lay out the `.S` hot sections in list order with the Rust ones
(xtask/src/hottext.rs numbering; check what the link does with `.text.sorted.h<n>` from the .S objects). Then the
string kernels sit with the probes that use them first, and adding keys no longer pushes them across a window. After
that, regen with the tool program as here. (b) cheaper: count the tool row on CI's host for origin/next and for this
branch (one job each). If next already reads 67-68 there, this branch's +1 is layout noise within the
HOTTEXT-VARIANCE rule ("highest count plus 2"), and the bound should be set by that rule with a recorded reason. Not
recommended: committing this regen as it is (strings test red on keke).
Not to redo: the regen run on keke (list.diff kept in ~/work/perf-readdir-entry-3/), the layout probe.

Run 1 jobs (keke, all rc 0): perf-readdir-entry-percall, PERF-READDIR-ENTRY-static, perf-readdir-entry-gate,
perf-readdir-entry-plants (3 record-walk plants CAUGHT). Run 1's total instructions per entry: readdir_entry_flat
Liella 1630.7 -> 1640.8, glibc 1636.6 -> 1640.3 (kernel getdents64 dominates; both sides moved by the same amount,
so treat it as noise); fts_entry_flat 6930 -> 6945 vs glibc 8044/8049; nftw_phys_1000 6271807 -> 6270280;
fts_physical_1000 and the walk rows unchanged within 0.1%. Run 1 counted totals only, not user-mode instructions.
Its `readdir64.dis` came out empty: readdir and readdir64 share one body, the symbol to read is `<readdir>`.

Run 2 change (8dee0d52): `<readdir>` was about 45 instructions on a hit, with `sub/add $0x18,%rsp` (the slow
path's KResult went through the stack), `mov $0x16,%eax` (EINVAL hoisted onto the hit path) and a register copy.
The exports `readdir`/`readdir64` now call `dirtree::readdir_addr(d) -> usize` (the address, 0 at the end or on
an error). Every miss (bad handle, lock_alone false, empty buffer, bad record) is one tail call to
`readdir_cold`, which checks the handle again, runs `readdir_slow` and sets errno only on an error. The record
walk (`DirStream::buffered_addr`) and its bounds checks are unchanged. Measured (perf-readdir-entry-2-percall, user-mode
instructions per entry, 3 runs per side, spread 0): readdir_entry_flat 50.293 -> 46.324, glibc 46.122 (ratio
1.09 -> 1.004; the 54.3 recorded in the register predates PERF-FSWALK-PHYS and the lock_alone swap). fts_entry_flat
1493.19 -> 1493.19 (glibc 793.6, unchanged). Totals unchanged within noise: opendir_readdir_closedir_1000,
fdopendir_99, scandir_alphasort_1000, nftw_phys_1000 6271364 -> 6269368, fts_physical_1000 6877174 -> 6876117,
fts_nochdir_flat_1000 6905129 -> 6917627 (+0.2%, kernel-dominated total; glibc moved by the same amount: 7960554 ->
7969779). The hit path is 41 instructions in after/readdir.dis. Plants: 3/3 CAUGHT (sym_readdir lines 1801 and
1804; fast-skips-record by s13_filesystem_clauses). Static (clippy -D warnings + xtask static): rc 0. Gate:
perf-readdir-entry-2-gate rc 0 (5 suites ok, 2192 passed, 0 failed; conformance 2/2).

Run 2 jobs (keke ~/logs/jobs.tsv; driver ~/work/perf-readdir-entry-2/driver.sh, log driver.log):
- perf-readdir-entry-2-percall: in n4, `patch -R` change.patch -> count.sh before -> `patch` -> count.sh after.
  Output in ~/work/perf-readdir-entry-2/{before,after}/: cbench.log, cbench_fstree.log (totals table),
  ctr.tsv (the binaries run directly with CBENCH_COUNTERS=1, 3 runs per side; the 7th field is user-mode
  instructions), readdir.dis.
- perf-readdir-entry-2-static: workspace clippy -D warnings, then `cargo xtask static`.
- perf-readdir-entry-2-gate: direct tier, --base origin/next.
- perf-readdir-entry-2-plants: RAM copy /dev/shm/claude/PERF-READDIR-ENTRY-2/plants-1. Plants: no-errno (cold
  path drops errno), end-sets-errno (the end sets errno), fast-skips-record (the hit path throws its record
  away). Tests: liella-boundary s13_filesystem_symbols (readdir filter) and liella s13_filesystem_clauses, each
  under timeout 600. Exits 1 on a survivor or a plant that did not apply.

Open / next step:
0. Bound done in run 4 (TOOL_BOUND 71). Results on 87b78a57 (mei): static rc 0, gate2 rc 0 (affected: ok, peak
   1.3G), plants rc 1: no-errno, end-sets-errno, tool-bound-low CAUGHT; **fast-skips-record SURVIVED** (all 4
   readdir symbol tests and 77 s13_filesystem_clauses pass; run 2 on keke reported it CAUGHT). Both hosts /tmp ext4.
   NEXT STEP: find out whether `buffered_addr` advances the position before returning (then the plant drops one
   entry per call and a test must see it: add a clause test that counts every entry of a known directory, sorted, no
   order dependence) or only peeks (then the plant is EQUIVALENT: write the reason, replace it with a plant that
   really drops a record). Rerun plants in a NEW zram copy (plants-2). Then CI. Separately HOTTEXT-ASM-ORDER (`.S` kernels in list order),
   then a regen with the tool program can bring the tool bound back down.
1. The readdir user-instruction gap is closed (1.004). The remaining fts_entry_flat gap (1.88x user) is not a
   readdir question: a separate story. The register paragraph in docs/saturation/17 records the counts.
2. What is left on the hit path (after/readdir.dis): null and alignment test of the handle (setcc/or form), the
   magic compare (movabs), the `Option<DirStream>` niche test, and in buffered_addr the `start <= end` compare
   of `get(base+pos..base+len)` (it cannot fail once pos < len, but LLVM cannot see that through wrapping_add)
   plus the `rec.len() >= 18` check. Slicing `buf.get(base..)` once and comparing the record length against
   `len - pos` could save 2-3 more, but that is a change to the record walk: rerun run 1's three plants on it.
Not to redo: the lock_alone swap (done, in force), the record-walk tests (done).
Plan rows to read: DU-3, MUTEX-2.
