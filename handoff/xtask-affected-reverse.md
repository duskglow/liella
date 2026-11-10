# XTASK-AFFECTED-REVERSE: `cargo xtask affected --reverse`

Branch `task/xtask-affected-reverse` (from `origin/next`). Lands through CI; not merged by hand.

## What changed

`xtask/src/affected.rs` only (plus this note and the plants file). The single CI train needs attribution: given the
tests that failed in a train and the files each merged branch changed, which branches' changes select each failed
test.

* One membership rule for both directions: `selects(sel, id) -> Option<Why>` answers "does this selection run this
  failed test, and why" (`Why::Full(reasons)` for an escalation to FULL, `Why::Rule(reason)` with the reason the
  forward selection recorded for the item). `covers` (used by `replay`) is now `selects(..).is_some()`. Ids are parsed
  once by `FailedId::parse` (target / lib / xtask / conformance).
* `reverse(repo, failed, branches, tier)` runs the forward selection itself (`select_tier`) per branch and asks
  `selects`; a branch that selects a test is explained file by file (each file's own `select_tier`), so the reason
  names the file and the rule. No mapping is duplicated.
* CLI parsing, `--help` (prints `USAGE` and `REVERSE_HELP`, the documented JSON shape), module header doc.

## How to call it

```
cargo xtask affected --reverse --failed 'liella::printf_products x' 'liella-mem y' \
    --branch fix-a=a.files --branch fix-b=b.files [--tier direct|wide|full]
cargo xtask affected --reverse --failed-from gate.log --branch fix-a=a.files ...
```

* Test ids are what the gate summary prints after `FAILED: ` (`crate::target name`, `crate name` for unit tests,
  `xtask::bin/xtask name`), or `conformance:<program>`. `--failed-from` reads every `FAILED: <id>` line and every
  `conformance FAILED: <program>: ...` line of a log or summary, so CI can pass the gate output through unchanged.
* A branch file list: one path per line, blank and `#` lines skipped.
* `--tier`: the tier the train gate ran (default direct, as forward mode).
* No header or xtask diff of the local tree is used (the lists are the branches' changes): a changed
  `csys/include` header or xtask source escalates to FULL, as forward `--files` does.

stdout, one JSON object (version 1):

```
{"version":1,"tier":"direct",
 "branches":[{"name":"fix-a","files":3,"full":false}],
 "failed":[{"test":"liella::printf_products x","kind":"target",
            "branches":[{"branch":"fix-a","confidence":"high","full":false,
                         "reasons":[{"file":"crates/core/src/printf.rs","rule":"<why>","full":false}]}]}],
 "unattributed":["<ids no branch selects>"]}
```

A branch that escalates to FULL selects everything: its reasons carry `"full":true` and a rule `FULL: <why>`; a claim
made only of FULL reasons is `"confidence":"low"`. A branch with a specific reason and an escalation keeps both reasons
and is `high`. stderr: a summary line per failed test (`test: fix-a (high: file); fix-b (low: FULL)` or `no branch
selects it`). Exit 0 when the attribution ran, 1 on an input error.

## Tests

Unit tests in `affected.rs`: `reverse_attributes_a_test_to_the_one_branch_that_selects_it`,
`reverse_reports_every_branch_that_selects_the_test`, `reverse_reports_a_test_no_branch_selects`,
`reverse_flags_full_escalation_as_low_confidence`, `reverse_agrees_with_forward_over_file_sets` (property-style: over
seven file sets, a one-branch reverse attributes a candidate id exactly when the forward selection's maps run it, and
every reason names a file of the set), `reverse_arguments_and_failed_ids`.

## Jobs (keke, role `~/w/afr`)

All on code commit 38f2329a (the later handoff commit changes only `handoff/`).

| Job | Command | Result | Log |
|---|---|---|---|
| afr-clippy-1 | `cargo xtask clippy` | rc 0 | `~/logs/afr-clippy-1.log` |
| afr-static-1 | `cargo xtask static` | rc 0 | `~/logs/afr-static-1.log` |
| afr-gate-1 | `cargo xtask affected --base origin/next --run` | killed (rc 143), see below | `~/logs/afr-gate-1.log` |
| afr-gate-2 | same, after restoring `fuzz/Cargo.lock` | rc 0: 2 suites, 283 passed, 0 failed (`affected: ok`) | `~/logs/afr-gate-2.log` |
| afr-smoke-1 | `cargo test -p xtask --bin xtask reverse` (6/6 ok) and a CLI run: `--help`, `--reverse` with `--failed-from`, `--failed` and three branches (JSON checked with `python3 -m json.tool`), `--reverse --run` refused (rc 1) | rc 0 | `~/logs/afr-smoke-1.log` |
| afr-plants-1 | `cargo xtask plants handoff/XTASK-AFFECTED-REVERSE-plants.txt --dir /dev/shm/claude/XTASK-AFFECTED-REVERSE/plants-1` | rc 0: 2 CAUGHT, 0 SURVIVED, 0 WARN | `~/logs/afr-plants-1.log` |

afr-gate-1 escalated to FULL: after the clippy and static jobs the role tree had a modified `fuzz/Cargo.lock` (31
added lines: `liella-boundary`, `liella-abi`, `liella-ld` entries), which `affected` reads as a changed build file. The
lock file on `origin/next` looks stale, and a build in the role rewrites it. I killed the FULL run by PID (it would
have taken a shared host for the whole `xtask test`), restored the file with `git checkout -- fuzz/Cargo.lock` and
reran as afr-gate-2. Any gate that runs `affected --base` after a build in the same tree is affected the same way:
a coordinator item, not part of this change.
