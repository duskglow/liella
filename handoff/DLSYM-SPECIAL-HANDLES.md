# DLSYM-SPECIAL-HANDLES (BLOCKED on DL-16; oracle delivered)

Branch task/dlsym-special-handles, cut from origin/next (c3424c99). Content head 5d2b70ba (this note and the
report JSON sit on top of it). Role shiki:~/w/sh2.

## State
The brief asks for a conformance program for `s27_1_c_3` (dlsym `RTLD_DEFAULT`/`RTLD_NEXT`) and `s27_1_c_1_3`
(`dlopen(NULL)`), proved by plants on Liella's `RTLD_NEXT` and `dlopen(NULL)` code. **That code does not exist.**
Liella has no `dlopen`/`dlsym` yet: `crates/ld/src/api.rs` and `crates/boundary/src/posix/s27_dynlink.rs` say
"Not yet implemented", `validation/results/libc-a-missing-manifest-symbols.txt` lists `dlopen` and `dlsym`, and
`conformance/run.sh` links only static programs (no helper shared objects). These clause tests are DL-16's own
proof (plan/06-phases/17-phase-14-dynamic-linking.md item 20: "Proof: s27_1_c_1*, c_3, ... clause tests"), and DL-16
is after DL-14. A test in conformance/c now would not link (a red suite); plants have nothing to mutate.

Delivered instead (commits c10cfdcc probe, 5d2b70ba behaviour note):
- `tools/probe/dl/handles/`: `handles.c` (the brief's self-checking program, written to become DL-16's
  conformance program), `handle.c` (helpers 1 and 2, both define `f`), `run.sh SCRATCH` (builds, runs on the
  reference loader, then runs two planted variants and exits non-zero if either survives).
- `spec/behaviour/dynlink.md`: a "Special handles" paragraph with the probe-settled behaviour and a DL-16 note.

## Probe-settled behaviour (glibc 2.39, black box)
RTLD_DEFAULT "f" = helper 1's f (= the program's own binding); RTLD_NEXT "f" from helper 1 = helper 2's f;
dlsym(dlopen(NULL, RTLD_NOW), "main_sym") = the -rdynamic program's main_sym; "f" through that handle = helper 1's.
Missing name: NULL + dlerror text, second dlerror NULL; prefix is the program for RTLD_DEFAULT and the NULL handle,
the calling object (libhandle1.so) for RTLD_NEXT.

## Jobs (shiki, final head)
- dlsym-special-handles-plants: probe real build rc 0 (fails=0); plant next-self CAUGHT (`n && n() == 2`), plant
  no-export CAUGHT (`m == &main_sym`). These plants are on the probe (they show the checks distinguish the right
  answer from the two wrong shapes the brief names), NOT on Liella code, which does not exist.
- dlsym-special-handles-gate: direct tier, 0 suites selected (probe + docs only), audits ok, rc 0.
- cargo clippy (workspace, all targets, all features, -D warnings) and cargo xtask static: see the report JSON.

## RULING NEEDED
- Re-sequence this story after DL-16 (or fold it into DL-16, whose proof it already is). Recommend: fold into DL-16;
  DL-16 adopts tools/probe/dl/handles/ as conformance programs and plants on its own RTLD_NEXT / dlopen(NULL) code.
- The conformance harness needs a dynamic-link mode with helper objects (a directive such as `LINK-HELPERS:`)
  before any §27 handle test can live in conformance/c. Recommend a small harness story ahead of DL-16's tests.

## Do not redo
The reference behaviour above is settled; rerun `sh tools/probe/dl/handles/run.sh <new scratch>` only to re-check.

## Next step
DL-16 (after DL-14): implement dlsym special handles, move handles.c/handle.c into conformance with the harness
extension, plant (a) RTLD_NEXT starting at the caller's own object, (b) dlopen(NULL) scope without the executable.
Plan rows to read: plan/06-phases/17-phase-14-dynamic-linking.md items 18-20; spec/behaviour/dynlink.md.
