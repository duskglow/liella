# DL-RELOC (phase 14 DL-10): eager relocation — DL-RELOC-3 state

**State:** branch `task/dl-reloc-3` (DL-RELOC-2's branch + origin/next merged 11-10 + this run), role kanon:~/w/kn1.
Decisions in force: D-27.17 (amends D-27.15 for empty PT_LOADs, #4258), D-27.11, D-27.15, R14-13, R14-8, R14-18,
L14-4, L14-6, L14-8, L14-12; rulings #4229 (open design question below), #4231 (done), #4259 (tests identify objects
by address range from the loader's link map), #4230 (COMPAT-19 is a later story).

## Done in DL-RELOC-3
* Why dl-reloc-2-plants2 was red: plant `irelative-not-deferred` (IRELATIVE applied in pass 1 too) SURVIVED. In the
  corpus, liba's IRELATIVE is the LAST entry of its pass 1, so the early call broke no ordering check, and nothing
  counted resolver calls. `reloc_model_corpus_one_object_per_type` now counts the relocations that need a resolver
  (each IRELATIVE + each reference bound to an STT_GNU_IFUNC definition) and asserts exactly that many calls (the
  mutant calls each IRELATIVE resolver twice). Plants file handoff/DL-RELOC-3-plants.txt (same 9, new msg).
* origin/next merged; `cargo xtask codemap` wrote nothing new; clippy + static green on the inherited head (lint0).
* docs/differences-from-glibc.md executable-stack row: "not yet implemented" replaced by what is in (startup
  refusal) and what comes with DL-16 (the `dlopen` refusal).

## #4229 (memory-image reader): NOT implemented: design question raised (RULING NEEDED)
The ruling stands (no /proc/self/exe re-read). Implementing it as "the reader over the live image" has a soundness
problem the ruling did not cover: the reader hands out `&[u8]` slices, and the relocator then stores into the same
memory through `ProcImage` (GOT/data in writable PT_LOADs; for a TEXTREL program, the text in read-only ones). A
live shared slice over bytes written through a raw pointer is undefined behaviour (and no SAFETY comment can state an
invariant for it). Files avoid it today because the file mapping is a separate private mapping.
Options (cost estimate for the next run):
* (a) RECOMMENDED: image mode in `Elf` with one slice per NON-writable PT_LOAD (file-backed part) plus the
  PT_DYNAMIC range, nothing else (all tables the relocator reads, dynsym/dynstr/hash/versions/RELA/JMPREL/RELR,
  live in read-only segments in every layout the toolchain emits); `segment_bytes`/`bytes_at` look up those
  slices by vaddr (elf.rs 543-585; the header and phdrs come from AT_PHDR, which the first read-only LOAD holds).
  `inside()` (reloc.rs 145) refuses a store that overlaps a sliced range for an image-mode object: one extra
  range test per store of the program only, no new pass. A kernel-started TEXTREL program with dependencies is
  then the one case not handled: refuse it with a diagnostic (a differences row: Liella refuses where glibc runs
  it) or copy its few tables into an anonymous mapping. Recommend refuse + row (binutils warns on DT_TEXTREL in
  PIE; no corpus program needs it). ~25 tool calls incl. a model test with a TEXTREL refusal and plants.
* (b) The reader reads through an accessor trait (no slices) for image mode: sound for every case, but it touches
  every reader path (elf.rs is 1830 lines) and costs instructions on all file-backed loads too. Not recommended.
* (c) Copy the program's read-only segments into an anonymous mapping and parse that: simple, but costs pages
  and instructions growing with program size (stressful-environments rule). Not recommended.

## Jobs (kanon)
* dl-reloc-3-codemap (no change), dl-reloc-3-lint0 (inherited head: clippy + static rc 0), dl-reloc-3-t1 (8/8).
* Final head: dl-reloc-3-gate2 (direct tier, 590 passed / 0 failed, static + clippy ok, peak 1.5G),
  dl-reloc-3-plants2 (9 plants on the final tests; irelative-not-deferred CAUGHT by "each IFUNC resolver runs
  once per relocation that needs it"), dl-reloc-3-counts (cbench_startup counters, see below).
* Superseded: dl-reloc-3-gate (green, older head), dl-reloc-3-plants (stopped: irelative-not-deferred survived
  again, cause below).
* Second cause found: the corpus had NO IRELATIVE: liba's IFUNC was global, so `a_ifunc_ptr = a_ifunc` links as
  R_X86_64_64 bound to an IFUNC. liba now also points at a local IFUNC (an IRELATIVE); the test asserts one exists.
* cbench_startup (instr/op, Liella vs glibc dynamic, Broadwell host): instr_c 2150 vs 119541, kinstr_c 157302 vs
  705104, setloc 67913 vs 145187, printf 71716 vs 146976, wctype 68940 vs 145406. Those programs are static Liella
  builds and never enter ld.so, so this story's code is not on these rows; no build of next was counted beside it.

## Not done / next step (DL-RELOC-4)
1. #4229 per the coordinator's answer to the RULING above, then drop the /proc refusal (boundary/src/ld.rs `link`,
   the only user of the program's file; `SELF_EXE` at ~352).
2. Count this code with a startup row that runs a dynamic program through Liella's ld.so (this run did not
   check whether cbench has one; cbench_startup's rows do not).
3. Arch seam checklist (from DL-RELOC): 1 ok, 2 ok, 3 n/a, 4 no new asm, 5 n/a, 6 ok, 7 tests cfg x86_64 (tagged),
   8 PROPERTIES + apply in the table, 9 n/a.

## Don't redo
The three t4 causes (empty PT_LOAD, unhashed symbols, --as-needed); dev/ino dedup; codemap; D-27.17 (amend it, do
not add another for the same); the resolver-count assertion.
