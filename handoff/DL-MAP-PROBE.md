# DL-MAP-PROBE: DL-MAP's open items (status: INCOMPLETE, budget checkpoint)

**State:** branch `task/dl-map-probe` (from origin/next ccc0d587f), role kanon:~/w/kn1. Commits: b84d12d67 (bound
removed, D-27.15 amended, tests), 4050f6302 (test fixes after gate3). Decisions in force: D-27.15 (amended here).

## 1. PT_LOAD bound (#4186): done in code, gate not yet green on the final head
* Probe (black box, kanon jobs `dl-map-probe-ref`, `dl-map-probe-ref2`, both rc 0; sources in the scratch dir
  `~/work/DL-MAP-PROBE/probe/` on kanon): a PIE and a shared object rewritten (program headers moved to a new
  page at the end, extra one-page read-only PT_LOADs) to 64, 65, 66, 100, 128, 200, 300, 500, 1000, 1100, 4000,
  10000 and 30000 PT_LOADs. The reference loader (Ubuntu glibc 2.39-0ubuntu8.9) loaded every one through
  `ld.so PROGRAM` (rc 0, program output) and `dlopen` (rc 0, symbol called).
* So `LOADS_MAX`/`MapError::TooManySegments` are removed (the brief's "remove it if the only real limit is the
  program header count"; the ELF reader bounds e_phnum and refuses PN_XNUM). D-27.15 amended, no differences row.
* Tests: the planner and the model mapper accept a 72-PT_LOAD synthetic object (the most whose headers fit in its
  one mapped page); the built ld.so runs a PIE rewritten to 300 PT_LOADs (`add_loads` in tests/map.rs), passes
  the maps checks and maps its last segment.
* INFORMATIONAL (kernel, not the loader): executing the rewritten programs directly failed (64: SIGSEGV, 65 and
  more: ENOEXEC, the shell then ran it as a script); the kernel's own program-header handling, out of scope.

## 2. Fuzz (#4188): running
`dl-map-probe-fuzz3` (kanon, started 09:46Z, 60 min, RAM copy /dev/shm/claude/DL-MAP-PROBE/tree of the tree with
the final map.rs; corpus /dev/shm/claude/DL-MAP-PROBE/fuzz-1, summary.tsv there). At 18.6M execs: cov 145, ft 471,
no finding. kanon had no cargo-fuzz: installed 0.13.2 (mei's version) in ~/.cargo/bin (job dl-map-probe-fuzz2).

## 3. Movable object, large p_align, first segment not at 0 (#4189): OPEN
`-fPIE -pie -Wl,-Ttext-segment=0x600000` (with 2 MiB page sizes) makes GNU ld emit **ET_EXEC**, not ET_DYN, so the
case was dropped (4050f6302). Next: build an ET_DYN another way (a linker script placing `SIZEOF_HEADERS` at
0x600000 with `-pie`, or `--image-base` if this ld supports it for ELF) and check e_type first; then add it to
`map_model_objects_from_the_linker` and to the maps test with align 0x200000.

## Gate history
* gate3 (head b84d12d67): 4 map tests failed: synthetic(1100) NoSegments (my test bug, fixed), align2m-at6m
  ET_EXEC (dropped), and `exec`/`zero-length` from a stale ld.so in the role (old message "only PIE programs load
  in this build"): the affected run did not rebuild the product. Clippy and static passed.
* gate4/plants were started while sync_role reported BUSY (the fuzz job's command line names ~/w/kn1), so they
  were stopped: SUPERSEDED.

## Exact next step
After `dl-map-probe-fuzz3` ends (~10:47Z): sync (rc must be 0), then run the gate as
`cargo xtask so && cargo xtask clippy && cargo xtask static && cargo xtask affected --base origin/next --run` and
plants `~/work/DL-MAP-PROBE/plants.txt` (one plant: re-adds a 64 bound; msg map_malformed_corpus_refused), then
item 3. Read: D-27.15, handoff/DL-MAP.md.

IMPROVEMENT: run fuzz jobs with no mention of the role path in their command line (copy the tree in a separate
job first), or sync_role keeps refusing for the whole fuzz hour.
