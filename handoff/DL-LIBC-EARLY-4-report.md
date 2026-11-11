# DL-LIBC-EARLY-4 report

Branch `task/dl-libc-early-4` (= DL-LIBC-EARLY-3's head d87c50040, then the commits below). Role mei:~/w/s4.
No product code changed in this run.

## Why -3 went red
`dl-libc-early-3-plants` (keke) rc 2 was a plants-file defect, not a code defect: the `rseq-tunable-ignored`
plant's replacement text `tunables.is_some() | tunables.is_none()` held ` | `, the field separator, so the tool
took a shell fragment (`tunables.is_none(), Relaxed); | cargo xtask so ...`) as its command and baseline-3 failed
(`sh: Syntax error: Bad function name`); no plant was applied. The WARN-DEV-SHM line was only a warning.

## Commits
* 6f345b809 handoff/DL-LIBC-EARLY-4-plants.txt: the same five plants; the tunable mutant is now
  `t.rseq != Some(false) || true` (no ` | `).
* 580af5800 merge of origin/next (28a903a01) into the branch (clean, no conflicts; brief note #4699).
* this report, handoff update and report json.

## Jobs (mei, all on the merged head 580af5800 unless noted)
* `dl-libc-early-4-lint`: `cargo xtask clippy`, `cargo xtask static`, `cargo xtask records check`: rc 0.
* `dl-libc-early-4-gate`: `cargo xtask affected --run` (direct tier, base origin/next) under `~/bin/gate 16G`.
* `dl-libc-early-4-plants`: `cargo xtask plants handoff/DL-LIBC-EARLY-4-plants.txt --dir
  /mnt/zram/claude/DL-LIBC-EARLY-4/plants-1` (RAM dir on the Skylake-SP host, not /dev/shm).
* `dl-libc-early-4-startup2`: after the gate, `cargo xtask bench --counters --only cbench_startup` on the head,
  then `cargo test -q -p liella --test hot_text_products` (merged head, #4699).
* `dl-libc-early-4-startup-next2`: the same counters on an origin/next 28a903a01 tree (git archive) in
  /mnt/zram, side by side on the same host (#4698).
* `dl-libc-early-4-startup-cmp`: waits for both, then fails if any start-up row's instructions ratio vs glibc on
  the head is above next's (script ~/work/DL-LIBC-EARLY-4/compare.sh on mei).

## Superseded
* `dl-libc-early-4-startup-next` rc 1 and `dl-libc-early-4-startup` (stopped): `bench --counters cbench_startup`
  is the wrong syntax ("unknown argument"); the form is `--counters --only cbench_startup` (as
  handoff/DL-LIBC-START-BIAS-4.md says). -3's startup script had the same mistake.

## Results
* lint rc 0; plants rc 0: 5 plants, 5 CAUGHT, 0 SURVIVED, 0 EQUIVALENT, no WARN lines.
* startup-next2 rc 0, next instructions vs glibc: c 0.017, printf 0.478, setloc 0.458, wctype 0.464, kinstr_c 0.221.
* The gate ESCALATED TO FULL (xtask/src/so.rs, a -3 change, is in the diff: xtask changes run the whole gate);
  gate, startup2 (after it) and startup-cmp were still running at hand-back: PENDING JOBS.

## Process notes
* One rule slip: an empty no-op `python3 - <<EOF` heredoc while reading the handoff (no effect on any file).
* Start-up commands for future runs: `cargo xtask bench --counters --only cbench_startup`; a next tree for side by
  side counts is `git archive origin/next` unpacked in a RAM dir on the build host.
