# DL-DLADDR-4 report

Branch `task/dl-dladdr-4`, role keke:~/w/n1. Brief: merge origin/next (DL-16c, DL-16d landed in the same loader files),
write the reserved ids, check the merged head. One-page state: handoff/DL-DLADDR.md.

## Commits (on top of task/dl-dladdr-3 61374f13a)

- `dabcd9b8b` Merge origin/next (b60c73ac6) into DL-DLADDR-4: five conflicts (ld.rs, runtime.rs, start.rs,
  docs/differences-from-glibc.md, spec/behaviour/dynlink.glibc.txt), all additive, both sides kept.
- `a1cb371fc` Read dlinfo's TLS answers from the published module table (RTLD_DI_TLS_MODID/TLS_DATA via
  `ld::initial_place` over `THREAD_MODS`, the source `dl_iterate_phdr` uses).
- `e2d5ea80e` Write the reserved ids: D-27.45..D-27.49, COMPAT-29.
- `2c16e89e1` Build the loader state for a first reader under next's STATE lock (the merged `read_section` still
  called the single lock DL-16c removed; found by the first lint run, `dl-dladdr-4-lint1`, E0425).
- `0461f75a5` Check dladdr as the process's first loader call (s27_dladdr), plants file.
- Then this report, the handoff and the json.

## Jobs (keke)

| label | head / tree | what | result |
|---|---|---|---|
| dl-dladdr-4-lint0 | inherited 61374f13a | step 0: `cargo xtask clippy` + `cargo xtask static` | rc 0 |
| dl-dladdr-4-lint1 | e2d5ea80e | clippy + static + records check | rc 1: liella-boundary E0425 `OWNER`, `lock`, `unlock` (fixed in 2c16e89e1) |
| dl-dladdr-4-lint2 | 2c16e89e1 | clippy + static + `cargo xtask records check` | rc 0; records: 608 decision, 29 compat, 0 pending |
| dl-dladdr-4-checks | 0461f75a5 | `cargo xtask so`; `cargo test -p liella-ld --test dlopen_concurrent` (DL-16c readers/fork); `--test tls` (DL-16d); conformance s27_dladdr, s27_dlinfo, s27_dlopen_tls_surplus | rc 0: 4 passed, 8 passed, conformance 3 passed / 0 failed; 27 min |
| dl-dladdr-4-plants | 0461f75a5 | `cargo xtask plants handoff/DL-DLADDR-4-plants.txt --dir /dev/shm/claude/DL-DLADDR-4/plants-1 --timeout 1500` | rc 0: 2 CAUGHT, 0 SURVIVED, 0 WARN, 14 min |
| dl-dladdr-4-gate | 0461f75a5 | `cargo xtask affected --run --base origin/next` under `~/bin/gate 16G` | escalated to FULL (compat_aliases.S is generated); running at hand-back |

Plants (both caught by their own msg; "caught by no test the log names" is the tool's note for a conformance
program, which has no Rust test name): `tls-place-no-bias` (dladdr.rs `tls_place` looks up the image without the
load bias) caught by s27_dlinfo's `RTLD_DI_TLS_MODID ... modid != 0`; `first-reader-refuses` (dlopen.rs
`read_section` refuses the first build) caught by the new s27_dladdr first-call check. The nine plants of run 3 cover
the rest of dladdr.rs; their sites are unchanged except the two TLS lines, whose literal patterns still match.

## INFORMATIONAL

- Two lock-free read paths exist after the merge: `ld::readers` (DL-16c's own table) and `ld::dlopen::read_section`
  (the chain: dlsym RTLD_NEXT, dladdr, dlinfo). DL-17's reader epochs must cover both or fold them.
- Before this run no conformance program made a reader (dladdr/dlinfo) the first loader call, so the lazy build in
  `read_section` was untested; the merge broke exactly that path (at compile time). Now covered.
- Earlier carried items (vDSO not in the chain; SERINFO of the program; ORIGIN of the program): handoff/DL-DLADDR.md.

## Refusals

None.
