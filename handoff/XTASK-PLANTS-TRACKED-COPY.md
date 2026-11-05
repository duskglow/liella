# XTASK-PLANTS-TRACKED-COPY (handoff, one page)

State: branch `task/xtask-plants-tracked-copy`, code commit a709399d (handoff commit on top). Role keke:~/w/n2.

## What the cause was (part 1)
The plant copy did NOT drop `catalogs_run.sh`: `rsync -a` copies everything but `/target` and `/fuzz/target`, and
the file was in `/dev/shm/claude/PERF-GETTEXT-LANGUAGE-STEADY-3/plants-2/src`. `catalogs_products` runs the script
through `crates/core/tests/oracle/nscache_mode.sh`, which mounts a fresh tmpfs over `/dev/shm` in a private mount
namespace. That hid the whole copy, because the prescribed plant dir on the Zen 1 host is under `/dev/shm`. The same
wrapper is used by about 14 products suites (patterns, mntent, process, fstree, syslog, netdb, posixipc, dns,
mallocdbg, terminals, aio, users, resolv, catalogs), so none of them could be planted from a `/dev/shm` copy.

## Changes (a709399d)
* `nscache_mode.sh`: binds back each top-level `/dev/shm` directory that holds `$PWD` or a path argument
  (`/dev/shm/x/..` or `NAME=/dev/shm/x/..`). The bind source is `/proc/$$/fd/3`, a descriptor opened on the old
  `/dev/shm` before the mount, and `mount --no-canonicalize --bind` is used. The shared region is a top-level file,
  so it still starts fresh.
* `cargo xtask plants`: after the copy, every source path must exist in the copy, or the run is refused with the
  missing paths (at most 20 are shown). The source paths are `git ls-files --cached --others --exclude-standard` in
  a git work tree, or every file and symlink outside /target and /fuzz/target in a synced role, which has no .git.
  A `--dir` under `/dev/shm` prints WARN-DEV-SHM: posixipc_run.sh's `nodev` phase mounts over `/dev` and cannot be
  bound back.
* `cargo xtask affected`, direct tier (part 2): the general cause is that the tier took spec sections only from the
  changed module and its one-hop `posix/sNN` users. The products tests name no module and carry only sections, so a
  helper reached through another internal module got none of them (intl -> boundary::catalog -> s07_locale
  `__dcgettext`). The tier now also takes the sections of the C entries whose bodies name the module or a one-hop
  user that is not a hub, an entry hub or test support. This applies to test targets only; conformance programs keep
  their call-based rule. intl.rs now selects `liella::catalogs_products` ("spec section 7 of a C entry the change
  reaches (`__dcgettext` ... names liella-boundary::catalog)"), and the direct tier went from 19 to 30 targets.

## Same cause, other files (one-off listing over crates/{core,boundary,legacy,math,kernel,rt,mem}/src)
* Products tests come only from the new rule: boundary ffi_sys (+29), stacks (+23), core scanf (+8), legacy argp
  (+6), fts (+3), getdate (+7), log_compat (+4), pthread_compat (+20), scanf_va (+8), rt pthread_sync (+6).
* Still no products test in the direct tier: boundary ffi_sys_aarch64, nr_aarch64, posix/s29_legacy, s29_rpc_auth,
  s29_rpc_clnt, s29_rpc_svc, s29_xdr_streams; core context, dl, signal, thread; kernel errno_list, vdso; legacy des,
  errno_slot, nis, sunrpc, sunrpc_clnt, sunrpc_rec, sunrpc_svc; math complex_body, fast32_tables, ulp, wide_dd;
  rt context, signal, unwind. Most likely they have no products test (aarch64, §29 RPC), or they are reached only
  through hubs. Not checked one by one.

## Tests and jobs (keke)
* New tests: `direct_tier_selects_the_products_tests_of_the_c_entries_a_helper_reaches`,
  `every_tracked_path_of_an_unusual_kind_is_copied` (a symlink, a dangling symlink, an executable, a dot dir, a name
  with a space and a newline, a nested `target/`, an untracked new file),
  `a_tracked_path_missing_from_the_copy_refuses_the_run` (a tracked file force-added under /target; refused before
  the build).
* clippy (`--workspace --all-targets --all-features -D warnings`) rc 0; `cargo xtask static` rc 0 (foreground, on
  a709399d).
* `xtask-plants-tracked-copy-plants`: rc 0, 5 CAUGHT / 0 SURVIVED / 0 WARN; copy check 4014 paths. Its baseline
  `catalogs_products` passed in a /dev/shm copy (the fix end to end); p-nscache-bind (no bind-back) was CAUGHT with
  "cannot open .../catalogs_run.sh".
* `xtask-plants-tracked-copy-gate`: `cargo xtask affected --run --base origin/next`. It ESCALATES TO FULL because
  rule 1 sends every xtask/ change except affected.rs and the audit modules to FULL. Left running for the dispatcher.
* `xtask-plants-tracked-copy-unit1` (an earlier head): superseded; it found a bug in my own test (an untracked
  target/stale with no .gitignore), fixed since.

## Open / rulings
* RULING NEEDED: a `plants.rs` change escalates to FULL. Recommend making it a leaf like `XTASK_AUDITS` (xtask unit
  tests and clippy only).
* RULING NEEDED: brief part 1 asked for a copy of exactly the tracked tree. The role has no .git, so the copy stays a
  superset (rsync -a); it is never smaller than the tracked tree, and the new check proves it. Recommend keeping it.
* INFORMATIONAL: posixipc_products still cannot run from a /dev/shm copy (its nodev phase mounts over /dev). Recommend
  a non-/dev/shm RAM dir on the Zen 1 host (keke has `/srv/ramtarget` tmpfs and no /mnt/zram) for plants on it.
* No D-row was written; the reserved ids were none. The rule text is in the affected.rs module doc and in
  `narrow_direct`'s doc.

Next step: the dispatcher reads the gate. If it is red, look only at failures in xtask or the oracle suites.
