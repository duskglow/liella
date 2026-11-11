# DL-TLS-SELF-IMAGE: ld.so's own scope entry from its mapped image (ruling #4501, D-27.23)

**State:** branch `task/dl-tls-self-image` (stacked on `task/dl-tls-static`, with origin/next 3b3845dc8 merged
in), role shiki:~/w/sh2. Decisions in force: D-27.23 (new, amends D-27.20), D-27.20, D-27.18, D-27.17; rulings
#4501, #4229, #4341.

## What changed
* **Merge of origin/next.** The story stacks on DL-TLS-STATIC (not on next) and uses DL-IMAGE-READER-3's
  image-mode reader (on next only), so origin/next is merged into the branch (ec6029985). The brief does not name
  the merge explicitly: flagged in the hand-back. Conflict in `crates/boundary/src/ld.rs` only: `link()` takes
  next's image view of the program (no program file read), keeps DL-TLS-STATIC's static TLS (`install_tls`, the
  interpreter as a scope object never relocated again), next's initialiser pass with the interpreter's hooks left
  out (it is not the loader's to initialise), and `enter(sp, entry, 0)` (next's `fini` argument) on the
  program-alone path. The interpreter is now also an edge in the DT_NEEDED order (`edge(q, n, name)`).
* **`own_object`** (ld.rs): reads ld.so's ELF header at `OWN_BASE` (its first `PT_LOAD`), takes `e_phoff` and
  `e_phnum` with `boot::own_headers`, and builds the scope entry through `image_view` with bias `OWN_BASE`
  (as D-27.20 did). Its three scratch mappings go into `views[3..]` and are unmapped with the program's at the
  end of `link`. ld.so opens neither the `PT_INTERP` path nor `/proc/self/exe`.
* The interpreter's `(dev, ino)` is no longer known: a dependency naming ld.so's file by PATH (not by soname) loads
  a second copy (DL-TLS-STATIC deduplicated it by file id). Same case as ruling #4341 for the program. INFORMATIONAL.
* Test `tls_interpreter_scope_entry_without_reading_its_file` (crates/ld/tests/tls.rs): the matrix program and
  libraries run through an execute-only (mode 0111) copy of ld.so, kernel-started and as `ld.so PROGRAM`; the old
  path fails both (EACCES on the PT_INTERP path and on /proc/self/exe). Skips with a message when an execute-only
  file is still readable (tests run with file-permission override, e.g. as root). This one test covers "no /proc"
  and "PT_INTERP unreadable" (no `unshare` needed).
* Plants: handoff/DL-TLS-SELF-IMAGE-plants.txt (file read restored; interpreter bias dropped).

## Start-up system calls (strace, shiki, `count.sh` in ~/work/dl-tls-self-image)
A PIE whose library uses GD TLS (DTPMOD64 present, so `__tls_get_addr` binds to ld.so's entry).
| run | before (DL-TLS-STATIC head) | after (this head) |
|---|---|---|
| kernel-started | 37: openat 4, close 4, fstat 3, lseek 3, mmap 9, munmap 3 | 33: openat 2, close 2, fstat 1, lseek 1, mmap 11, munmap 5 |
| `ld.so PROG` | 50: openat 5, close 5, fstat 3, lseek 4, mmap 15, munmap 4 | 46: openat 3, close 3, fstat 1, lseek 2, mmap 17, munmap 6 |
The "before" tree also read the program's file (next's image reader removes that), so the change covers two
objects. In both modes the open of the interpreter's own file is gone (per object: openat, fstat, lseek, close
fewer; one scratch mmap/munmap pair more, net -2 calls). Split by object inferred from the traces, not measured
against next + DL-TLS-STATIC with the file read.

## Results
See the report section of the hand-back and handoff/reports/DL-TLS-SELF-IMAGE.json. dl-tls-self-image-t2 (shiki):
clippy, static, so rc 0; tls 5/5, reloc 14/14; count after. dl-tls-self-image-t1 superseded (unsafe-audit: a
SAFETY note grew past the audited span; shortened in 3rd commit). dl-tls-self-image-count-before: the before row.

## Next step
None required for this story. Follow-ups: the path-named-interpreter duplicate (above); the image view's extra
scratch mapping per object could share one mapping for the header table and PT_DYNAMIC copy.
