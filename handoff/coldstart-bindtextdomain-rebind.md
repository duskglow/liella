# COLDSTART-BINDTEXTDOMAIN-REBIND: second bindtextdomain + dgettext, same directory

State: branch `task/coldstart-bindtextdomain-rebind`, stacked on next e8189dd8 (contains 730f416b). No code change:
the brief's hypothesis is refuted by the code and by a probe. Result: INCOMPLETE (the row is still slow; its cause is not found).

## Step 0: re-time (read, not re-run)
nozomi:CSP1106-n100 (DONE, staged 730f416b, built on kanon, N100): `second_bindtextdomain_gettext` Liella 8899 ns/op.
That partial run reports no glibc figure for the cold rows. The 7.7x (N100) / 7.0x (Ivy Bridge) in the brief come from
TIMING-SWEEP-1105 (next f2157504, results/timing-sweep-1105-table.md:303). The row is not at or below 1.05x on any
measurement found, so step 0 does not close the story. NOT VERIFIED: a fresh glibc-side time on 730f416b or later.

## The hypothesis is already implemented (D-7.12)
`crates/legacy/src/intl.rs` `bind()`: when the new value equals the bound text (compared by content, `text(h, rec, slot) ==
value`), it returns early with no `bump()`. That is the owner ruling of 2026-10-23 (#1056, D-7.12). With no new generation,
`catalog_of` → `unchanged` returns from `C_CHECKED_GEN == generation && !recheck::due` without a file-system check.
`setlocale` to the same name keeps the same global handle (`global_locale::intern_locked` finds the existing view), so
the bench's second `setlocale_en_US` row does not bump the generation either.

## Probe (mei, build of this branch's tree, `cargo xtask so` 02:08)
Probe source: mei `~/work/COLDSTART-BINDTEXTDOMAIN-REBIND/rebind.c` (copy in the agent scratch). Sequence: setlocale
en_US.UTF-8; bindtextdomain + dgettext("cbench","hello"); dgettext on an absent domain; setlocale again; then the
second bindtextdomain (same directory) + dgettext, between two marker-page touches and two getrusage calls.
- Liella (static) and glibc (dynamic): the translation is correct ("hallo"). ru_minflt delta = 2 = the two marker pages
  only, 3 runs each. Over the second pass: **0 page faults and 0 syscalls** on both (strace between the two getrusage
  calls is empty; `perf record -e page-faults -c 1 -d`: no sample between the markers).
- Spec gap (re-read of a .mo changed on disk at a rebind): not needed. The code does not re-read on a same-value rebind
  (D-7.12 decides it: the one-second recheck clock still applies), so this story raises no gap.

## What is open (next step)
In this minimal sequence the second rebind is free. So the brief's evidence (+968 user, +25k kernel, +2 faults for the
second "B"; Heap::refill and machine_memset at the top of the profile) comes from the bench's other rows between the two
passes (strcoll, iconv, localtime, strftime, mktime, regcomp, fnmatch, wordexp), not from the rebind logic.
Candidate (NOT VERIFIED): the lookup path allocates (for example a per-lookup resolution record) and, after the
interleaved rows used up the heap's current page, that allocation faults a fresh page (refill + zeroing). Alternatively the
+25k kernel / +2 faults are noise between separate processes (the neighbouring prefixes differ by ±3k ik).
Exact next step: run the bench's own cold child (`cbench_opt` cold mode, rows in bench order) under `perf record -e
page-faults -c 1 -d`, with marker-page touches around row 11 of round 2 (a scratch copy of cbench_opt.c), and attribute
the faults by symbol. If an allocation in `intl::translate` on a repeated lookup is the cause, remove it on that path.
Also count allocations per repeated dgettext (a malloc counter in the probe).

Things not to redo: the "keep the catalog on a same-directory rebind" change (it exists); the setlocale-bump theory (same
handle, no bump).

Plan rows to read: D-7.12 (`grep -rn "^| D-7.12 " plan/`).

## Jobs
- counts / profile: mei:coldstart-rebind-probe (probe above; ad hoc ssh in the role dir ~/w/s1, nothing left running).
- timing: read nozomi:CSP1106-n100 (the planning story's run, not started by this task).
- gate, plants: not run: no code change.
