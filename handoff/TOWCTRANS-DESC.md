# TOWCTRANS-DESC: O(1) `wctrans_t` decode for `towctrans` (D-5.8; WCTYPE-RETHINK D-NEW-4)

**State:** branch `task/towctrans-desc` from origin/next `70cb44d76`; HEAD in handoff/reports/TOWCTRANS-DESC.json.
Build role mei:~/w/s1. Jobs (mei `~/logs/jobs.tsv`): `towctrans-desc-gate` (affected --run --base origin/next),
`towctrans-desc-plants` (handoff/TOWCTRANS-DESC-plants.txt), `towctrans-desc-counts` (~/work/TOWCTRANS-DESC/count.sh: waits for
the gate, then `wcprobe count` Liella/glibc and `cargo xtask bench --compare --counters cbench_locale`; outputs in
mei:~/work/TOWCTRANS-DESC/). Results: see "Results" below (filled at hand-back; PENDING where a job was still running).

## The change
* `CtypeFile::case_desc(upper)` (crates/core/src/locale/ctype.rs): the one definition of a compiled locale's `toupper`/`tolower`
  `wctrans_t` (case table address at character 0). `trans_desc` (legacy) now calls it.
* `LocaleView` stores `case_descs: [usize; 2]` when built (compiled LC_CTYPE only); `case_descs()` returns `None` for the
  built-in views, whose descriptors are the built-in `C_TOUPPER`/`C_TOLOWER` addresses (shared by C and C.UTF-8, as before).
* `ctype_tables::towctrans_view(wc, desc, v)`: one compare against each descriptor, then `v.wtrans(wc, upper)` (the code
  `towupper`/`towlower` run, inlined once); anything else goes to `towctrans_other` (out of line) = the old search
  (locale-defined maps such as C.UTF-8 `totitle`, another locale's descriptor, null, garbage -> identity).
* The four exports `towctrans`, `__towctrans` (thread view), `towctrans_l`, `__towctrans_l` (`view_of(loc)`) use it.
* No observable change: answers equal the old ones by construction (same descriptors, same case maps as `towupper`).

## Test and plants
* `crates/boundary/tests/towctrans_desc.rs` (Trace s5_3_c_0, D-5.8): every code point 0..=0x10FFFF plus WEOF and 3 values beyond
  Unicode, `towctrans(c, wctrans(name))` == `towupper`/`towlower` in C, C.UTF-8 and en_US.UTF-8 (skip note when absent), all four
  entries (the `_l` ones while the thread is in a different locale); null descriptor = identity; C.UTF-8 `totitle` via the
  general path; descriptor binding across `uselocale` (en_US's in C maps to itself, C's in en_US maps to itself, both work again).
* Plants (7): swapped view descriptors, swapped built-in compare, inverted map direction, built-in views offering zero
  descriptors, other maps dropped, `towctrans` reading the global view (stale after `uselocale`), `towctrans_l` reading the thread's.

## Decisions in force
D-5.8 (this story), D-5.7, D-7.13 (views). Reserved id D-5.8 used as the brief says.

## Not to redo / next
* Timing on Ivy Bridge and N100: PENDING (timing hosts down from 11-07 20:00 UTC until about midweek). Use the rethink's
  `handoff/wctype-rethink/trun.sh` with the towctrans rows.
* Next in the rethink: WCTYPE-TRIE-BUILTIN, then WCTYPE-TRIE-FILE (handoff/wctype-rethink.md section 8).

## Results (final head 20f282401, mei)
* Gate `towctrans-desc-gate2` (affected --run --base origin/next): 8 suites ok, 7174 passed, 0 failed; peak 13G, rc 0.
  clippy and static clean on the role.
* Plants `towctrans-desc-plants2`: 7 CAUGHT, 0 SURVIVED, 0 WARN.
* Counts `wcprobe count` (net instructions per call, ranges ascii/latin1/cjk/bmpmix/astral/bmprnd; before = the rethink's
  `count-liella-noproto.tsv`, same probe build, base acc8cfcd5; glibc = installed glibc, same job):

| locale | towctrans before | towctrans after | towupper (after) | glibc towctrans |
|---|---|---|---|---|
| C | 71/62/62/62/62/62 | 26/21/21/21/21/21 | 20/15/... | 20/7/7/7/7/7 |
| C.UTF-8 | 76/95/95/95/95/95 | 29/48/48/48/48/48 | 23/40/... | 23/23/19/20/17/19 |
| en_US.UTF-8 | 319/454/445/448/443/445 | 19/19/47/50/44/47 | 16/16/44/47/41/44 | 23/23/19/20/17/19 |

* A first version (one arm, `upper` as a value: `towctrans-desc-gate`/`-plants` green, superseded) measured en_US cjk 84:
  the inlined walk kept both case tables live (6 saved registers, objdump of Liella's probe binary). Two constant-direction
  arms fixed it (commit 20f282401).
* `cbench_locale --compare --counters`: in `towctrans-desc-counts2` (mei:~/work/TOWCTRANS-DESC/cbench_locale.log), still
  running at hand-back.
* Timing: PENDING (timing hosts down). Remaining gap to glibc above Latin-1 is the case walk itself (rethink item 4).
