# WL-PREP-RELEASE-PROFILE report

Commits (branch `task/wl-prep-release-profile`, all new, none amended):
1. `580ae0c29` Build the workload products with the shipped release profile (wl_lock.sh, every consumer, self-test,
   affected mapping).
2. `af29af18f` Record the release-profile decision and its plants (pending D-10 record, plants, one more self-test case).
3. This report, the handoff note, the corrected msg of plant wlrp-no-cache-check.

## Jobs (shiki, role sh2)

* `wl-prep-release-profile-lint`: `cargo xtask clippy && cargo xtask static`, rc 0 (head af29af18f).
* `wl-prep-release-profile-gate`: `cargo xtask affected --run --base origin/next` on af29af18f, rc 0: 2 suites,
  371 passed, 0 failed; peak 1.3G.
* `wl-prep-release-profile-plants`: plants-1 on af29af18f: wlrp-no-switch, wlrp-sha-key, wlrp-no-build-check CAUGHT
  with their msg; wlrp-no-cache-check caught (rc 1) but WARN-NO-MSG: its msg named the wrong check (that plant reuses
  a fast set under a release key; the legacy `<sha>` set is not under the release key, so a build still runs). The log
  shows `FAIL fast set under a release key refused (rc): expected [1] got [0]`; the msg now names that line.
* Final head: `wl-prep-release-profile-gate2` (clippy, static, then the affected gate) and
  `wl-prep-release-profile-plants2` (plants-2); results in the hand-back.
* `wl-prep-release-profile-count`: the Fast-vs-Release count below, rc 0.

## Fast vs Release at one sha (af29af18f, Haswell host, instructions:u per call, `--base none`)

| case | glibc | Liella fast | Liella release | release - fast |
|---|---:|---:|---:|---:|
| mbrtowc_ascii | 211.2 | 42.0 | 42.0 | 0 |
| mbrtowc_2byte | 234.2 | 56.0 | 56.0 | 0 |
| printf_short | 862.3 | 882.6 | 802.5 | -80.1 |
| fgets_line | 200.6 | 206.3 | 202.6 | -3.7 |
| fwrite_short | 150.7 | 94.9 | 93.9 | -1.0 |

Products: `~/wl/products/af29af18f...` `.profile=fast`, `~/wl/products/af29af18f...-release` `.profile=release`. The
release products change the stdio rows (printf_short no longer above glibc). **mbrtowc does not change** at this sha:
the brief expected Release to lose a GOT read there; that was not observed in the counts, and three attempts to show
the read in a disassembly found no `mbrtowc` body by name in the archive or the linked binaries (alias/weak symbol),
so the per-read claim for mbrtowc is NOT VERIFIED here (it may be absent from mbrtowc's typical path at this sha).

## Notes

* INFORMATIONAL: a primed base whose only cached products are the old fast set is no longer "present" (presence checks
  look for `<sha>-release`); its source is needed again (`wl_prime_base.sh`), and the first count of each commit builds
  fat LTO (minutes longer). Old fast caches are never deleted; `WL_PROFILE=fast` reads them.
* The path changes in the lab drivers (wl_gate, wl_count, wl_run, wl_attr, wl_alloctrace*, wl_prime_base, mcs_run)
  have no gate self-test; `bash -n` clean; wl_percall.sh / wl_percall_run.sh / wl_lock.sh / wl_local.sh ran for real in
  the count job.
* Process slip: one shell call held an empty `python3 - <<'X'` heredoc stub (no effect); all other edits used files.
* No refusals.
