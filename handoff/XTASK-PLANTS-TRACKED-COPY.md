# XTASK-PLANTS-TRACKED-COPY (handoff, one page; runs -1 and -2)

State: branch `task/xtask-plants-tracked-copy-2` (stacked on `task/xtask-plants-tracked-copy`). Code head 6cf21394
(this handoff commit on top). Role keke:~/w/n1. Spec: the original brief's "The change" plus rulings #2993 and #2994.

## Why catalogs_run.sh was "missing" (ruling #2994)
It was never missing from the copy. The copy is `rsync -a` that excludes only `/target` and `/fuzz/target`. There is
no extension filter, no `.sh` filter and no `oracle/` filter, and the copy check found all 4014/4017 source paths in
both runs. `catalogs_products` runs its script through `crates/core/tests/oracle/nscache_mode.sh`, which mounts a
fresh tmpfs over `/dev/shm` in a private mount namespace. That hid the whole copy, which lives under `/dev/shm` on
the Zen 1 host. The cause is fixed in nscache_mode.sh (the bind-back below), so the copy check never has to fire for
it. The copy stays a superset (`rsync -a`) of the tracked tree: roles have no .git (#2994, accepted).

## Changes
* -1 (a709399d): nscache_mode.sh binds back each top-level `/dev/shm` dir holding `$PWD` or a path argument, from a
  descriptor on the old `/dev/shm`. `cargo xtask plants` refuses a copy that is missing any source path. The direct
  tier also takes the spec sections of the C entries whose bodies name the changed module or a non-hub one-hop user,
  so intl.rs selects `catalogs_products`. The other files the same cause left unmapped are listed in the -1 report
  (git history of this file).
* -2 (6cf21394): fixes -1's red gate. The six `aio_products` failures came from THIS branch, not from next. The log
  said `mount: /dev/shm/liella-aio-...: special device /proc/N/fd/3/liella-aio-... does not exist`, and that bind
  exists only in this branch's nscache_mode.sh. aio passes a scratch path under `/dev/shm` that does not exist yet:
  it is meant to be created inside the private /dev/shm. Now only a directory that already exists in the old
  /dev/shm is bound back (`[ -d /proc/$$/fd/3/$n ] || continue`). So I did not run next.
* -2, ruling #2993: `xtask/src/plants.rs` is in `XTASK_AUDITS`; its only user is main.rs. New test
  `a_plants_change_selects_exactly_the_xtask_tests_and_clippy` checks no FULL, xtask tests, xtask clippy only, no
  products, no conformance and no commands.

## Jobs (keke, on 6cf21394)
* `xtask-plants-tracked-copy-2-gate`, rc 0: direct tier (`affected --run --base origin/next`), no escalation. 13
  products targets came in through nscache_mode.sh (aio, catalogs, dns, fstree, mallocdbg, mntent, netdb, patterns,
  posixipc, process, syslog, terminals, users), plus the xtask unit tests. 20/20 products tests passed (6/6 aio);
  peak 1.2G. Separately, `affected --list --files crates/legacy/src/intl.rs` selects `liella::catalogs_products`
  ("spec section 7 of a C entry the change reaches (`__dcgettext` ... names liella-boundary::catalog)").
* `xtask-plants-tracked-copy-2-plants`, rc 0: dir /dev/shm/claude/XTASK-PLANTS-TRACKED-COPY-2/plants-1; 7 CAUGHT /
  0 SURVIVED / 0 WARN-RC-ONLY / 0 WARN-NO-MSG; copy check 4017 paths. Plants file
  handoff/XTASK-PLANTS-TRACKED-COPY-2-plants.txt (the 5 from -1 plus p-plants-leaf and p-nscache-exists). The
  aio_products baseline passing in a /dev/shm copy is the end-to-end proof of the fix.
* Foreground on the role: `cargo clippy --workspace --all-targets --all-features -D warnings` rc 0, `cargo xtask
  static` rc 0, `cargo test -p xtask -- affected::tests plants::tests` 55 passed.
* Superseded: `xtask-plants-tracked-copy-gate` (-1, FULL, rc 1: the aio defect fixed above),
  `xtask-plants-tracked-copy-plants` (-1, older head).

## Open
* INFORMATIONAL (from -1): posixipc_products cannot run from a /dev/shm plant copy, because its nodev phase mounts
  over /dev. For plants on it, use a RAM dir outside /dev/shm (keke has /srv/ramtarget).
* No D-row (no reserved ids). The rules are in affected.rs's module doc and nscache_mode.sh's header.

Next step: none for this story. The dispatcher reads the two -2 jobs.
