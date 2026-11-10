# Fuzzing

Liella has libFuzzer harnesses for every parser of untrusted input: 20 targets in `fuzz/fuzz_targets/`
(implementation-plan.md §6, "Fuzzing infrastructure"). They form a workspace of their own (`fuzz/`, nightly toolchain named in
`fuzz/rust-toolchain.toml`) outside the product build. Each harness calls Liella's parsers through their internal API on bytes
in memory. No harness opens a system file, runs a command or uses the network.

| Target | Parser |
|---|---|
| nsswitch, resolv_conf | `nsswitch.conf`, `resolv.conf` |
| hosts, passwd, group, shadow, netdb_files | the files databases (`hosts`, `passwd`, `group`, `shadow`; `services`, `protocols`/`rpc`, `networks`, `aliases`, `netgroup`) |
| nscache | the name-service cache's shared region (any content another process wrote) |
| dns | DNS messages |
| digits_dots | numeric host names (`inet_aton`'s dotted forms, IPv6 literals) |
| regex | POSIX regex compile and match |
| regex_dfa | the lazy DFA's answer against the matcher's |
| printf, scanf | format strings and conversions |
| locale, charmap | locale source and charmap files |
| iconv | character-set conversion |
| glob, wordexp | glob patterns, word expansion (no commands are run) |
| scanners | the C layer's string scanners (`strcmp`, `strlen`, `strnlen`, `getenv`) at page ends, against Rust references |
| elf | the dynamic loader's ELF reader (`liella_ld::elf`, D-27.7, D-27.8): headers, notes, the dynamic section, both hash tables, the symbol version tables, RELA/RELR |
| ld_map | the dynamic loader's mapper (`liella_ld::map`, D-27.15): segment and RELRO validation at 4, 16 and 64 KiB pages, and the reservation, alignment and `.bss` arithmetic of `map::load` against a mapper that checks each call lies inside the reservation and is never writable and executable; seeds: a copy of the `elf` seeds |

The `elf` seeds (`fuzz/seeds/elf`) are small stripped objects linked with `-nostdlib`, so no C library start files are in
them (the shared objects' files have no `.so` suffix, which the repository ignores): a versioned library `dep`; three libraries
that define versions (one hidden), need `dep`'s versions and carry
TLS, PLT and data relocations, linked with both hash tables and RELR, with the SysV hash only, and with the GNU hash only and
`-z now`; a library without notes; and a PIE with `PT_INTERP`.

## Running

```
cargo xtask fuzz --list
cargo xtask fuzz <target> --time <minutes>      # chunks of a fresh libFuzzer process, resuming from the corpus
cargo xtask fuzz <target> --repro <file>        # run one input (a crash artifact)
cargo xtask fuzz <target> --tmin <file>         # minimise a crashing input
cargo xtask fuzz <target> --cmin                # minimise the corpus
```

The corpus lives outside the repository, in `$LIELLA_FUZZ_CORPUS/<target>/corpus` (default `the fuzz corpus directory`). Crash
artifacts go beside it in `artifacts/`. The seeds in `fuzz/seeds/<target>` are read as a second corpus and are never written.
The build is release with debug assertions and no sanitizer, so integer overflow panics as in the tests. Findings are panics,
time-outs and memory-limit reports. The per-target limits (input size, time-out, memory, chunk length) are in
`xtask/src/fuzz.rs` (`settings`).

## Campaign PRE14-FUZZ-CAMPAIGN (V-3, 2026-10-23)

Method: every target for a fixed **30 minutes**, one libFuzzer process each, on the Zen 1 host. The campaign used a RAM copy of the
tree (`/dev/shm`) with the corpus also in RAM. That corpus started as a copy of the corpus the earlier Zen 1 host campaigns
(2026-10-09 to 10-11, 20 min per target) left in `the fuzz corpus directory`. The work ran as two queued 7-core jobs of
10 targets each, 7 fuzzers at a time:

* `pre14-fuzz-campaign-a`: nsswitch resolv_conf hosts netdb_files passwd group shadow nscache dns digits_dots
* `pre14-fuzz-campaign-b`: printf scanf charmap iconv wordexp scanners regex regex_dfa glob locale

The script is `<work dir>/campaign.sh` on the Zen 1 host. Each job writes its results to
`<work dir>/run-<stamp>-<a|b>/`: `results.tsv` (one row per target), `<target>.stats` (libFuzzer's final
stats) and `artifacts/<target>/` (every crash, time-out or OOM input). Under `xtask fuzz`, a target's first finding ends that
target's run. The job exits 0 when every target ran to its time or stopped on a recorded artifact. It exits non-zero when a
target fails without an artifact (a build or harness failure).

### Results

Recovered 2026-11-04 (PRE14-FUZZ-RESULTS) from the two `results.tsv` and `.stats` files in `~/work/pre14-fuzz-campaign/` on the lab VM that ran the campaign
(jobs `pre14-fuzz-campaign-a`, 2026-10-23 06:55:48Z to 07:59:26Z, and `-b`, 06:58:39Z to 07:31:42Z, both rc 0). The campaign ran on the
tree of the PRE14-FUZZ-CAMPAIGN branch on 2026-10-23; the head sha was not recorded in the results files.

**Coverage was not recorded.** The campaign script kept only libFuzzer's final stats, so the `cov`/`ft` column of `results.tsv` is `?`
for every target. Per-target coverage is still open: `cargo xtask fuzz` now keeps the last libFuzzer status line of each
target's run (across chunks) and ends with one row per target, `<target> cov=<n> ft=<n> corpus=<n> execs=<n>`; `--summary <file>`
writes the same rows as TSV (`target cov ft corpus execs`). A run that printed no status line (a crash before the first one) says
`none` in every column that depends on it, never 0. The rerun that fills the column is the next section.
Corpus size, executions and peak RSS are as recorded. Time is in seconds (1801-1803 s is the 30 minute budget); a target with a
finding stopped at its first finding.

| Target | Job | Time | Corpus (start -> end) | Executions | Peak RSS (MB) | Finding |
|---|---|---|---|---|---|---|
| group | a | 1802s | 21329->22759 | 32,986,389 | 44 | none |
| shadow | a | 1802s | 21237->22394 | 50,292,652 | 41 | none |
| passwd | a | 1802s | 18209->18488 | 87,050,577 | 37 | none |
| netdb_files | a | 1802s | 31186->32219 | 57,636,207 | 62 | none |
| nsswitch | a | 1802s | 27601->28795 | 35,655,834 | 54 | none |
| hosts | a | 1802s | 26909->28900 | 13,202,374 | 42 | none |
| resolv_conf | a | 1801s | 67989->70539 | 65,557,510 | 59 | none |
| nscache | a | 1802s | 4576->4707 | 32,524,699 | 32 | none |
| digits_dots | a | 1802s | 847->848 | 77,175,933 | 36 | none |
| dns | a | 1802s | 5073->5361 | 53,460,943 | 36 | none |
| regex | b | 762s | 4450->5308 | 103,103 | 199 | FINDING: time-out |
| regex_dfa | b | 67s | 2698->2698 | 2,410 | 118 | FINDING: time-out + slow unit |
| glob | b | 367s | 3124->5315 | 546,982 | 1192 | FINDING: time-out |
| locale | b | 232s | 2135->2773 | 365,301 | 1003 | FINDING: time-out |
| scanners | b | 1801s | 394->422 | 174,042,781 | 28 | none |
| iconv | b | 1801s | 19893->22991 | 47,038,225 | 54 | none |
| scanf | b | 1801s | 19488->22666 | 82,313,632 | 37 | none |
| wordexp | b | 1801s | 26091->35221 | 11,660,822 | 54 | none |
| printf | b | 1802s | 20469->22740 | 1,176,367 | 38 | none |
| charmap | b | 1803s | 13491->14781 | 618,398 | 258 | none |

Sixteen targets ran their full 30 minutes clean; four (regex, regex_dfa, glob, locale) stopped on a time-out artifact.

### Coverage rerun (FUZZ-COVERAGE-STATUS-2, 2026-11-04)

One libFuzzer process per target for a fixed 30 minutes, 13 at a time, on a 26-thread host in RAM (`/dev/shm`), through
`cargo xtask fuzz <target> --time 30 --summary <file>` on the head of branch `task/fuzz-coverage-status-2`. The corpus started
empty (only the repository seeds), so corpus sizes and execution counts are not comparable with the table above. `cov` and `ft` are
the last libFuzzer status line of the target's run. Twenty-six targets ran (six were added since the 2026-10-23 campaign:
rpc_call, rpc_stream, fnmatch, collate, xdr_mem, xdr_rec).
Two stopped on a finding; their coverage is the value at the stop.

| Target | cov | ft | Corpus | Executions | Finding |
|---|---|---|---|---|---|
| charmap | 1675 | 8562 | 4877 | 826,217 | none |
| collate | 215 | 600 | 171 | 89,322,502 | none |
| digits_dots | 553 | 1633 | 597 | 68,036,943 | none |
| dns | 1100 | 4386 | 1299 | 23,758,064 | none |
| fnmatch | 965 | 5667 | 2568 | 5,637,267 | none |
| glob | 1580 | 10721 | 3615 | 2,346,987 | none |
| group | 214 | 819 | 87 | 1,807 | FINDING: crash, harness assertion "each GID once" (`users::group_list` returned a GID twice); stopped after 1,807 executions |
| hosts | 556 | 2677 | 1562 | 7,662,783 | none |
| iconv | 807 | 5165 | 2798 | 19,571,443 | none |
| locale | 1862 | 4473 | 2495 | 588,442 | none |
| netdb_files | 432 | 3234 | 1340 | 15,727,392 | none |
| nscache | 332 | 1300 | 347 | 11,028,834 | none |
| nsswitch | 364 | 2496 | 1448 | 12,073,430 | none |
| passwd | 179 | 1206 | 534 | 15,765,357 | none |
| printf | 1478 | 9763 | 4052 | 2,779,085 | none |
| regex | 2447 | 12713 | 2597 | 257,210 | none |
| regex_dfa | 2422 | 10698 | 1211 | 25,065 | none |
| resolv_conf | 667 | 4244 | 2763 | 22,993,573 | none |
| rpc_call | 80 | 107 | 41 | 698,423,641 | none |
| rpc_stream | 96 | 453 | 203 | 34,872,782 | none |
| scanf | 1184 | 5601 | 2352 | 48,168,082 | none |
| scanners | 115 | 337 | 87 | 155,322,241 | none |
| shadow | 242 | 1846 | 720 | 12,179,563 | none |
| wordexp | 1746 | 8765 | 2602 | 295,601 | none |
| xdr_mem | 515 | 1768 | 377 | 150,895,486 | FINDING: out-of-memory (2048 MB limit) after 150,895,486 executions; the stored input (empty) runs clean alone, so memory grew across executions |
| xdr_rec | 148 | 637 | 167 | 24,238,032 | none |

A finding is a defect with its own fix story (group: `group_list` duplicate GID; xdr_mem: memory growth); neither is fixed here.
The four 2026-10-23 time-outs (regex, regex_dfa, glob, locale) did not stop their targets in this run's 30 minutes (the run starts from an empty corpus, so this is not a replay of those inputs). Low execution counts for regex (257,210) and
regex_dfa (25,065) show those targets are slow per input, not stopped.

### Findings

Each finding (crash, hang/time-out, OOM) is a defect and gets its own fix story, not a rerun. Reproduce it with
`cargo xtask fuzz <target> --repro <artifact>` and minimise it with `--tmin`. A finding that may be a security weakness is not
described here (owner rule SECURITY-PRIVATE).

The 2026-10-23 campaign found four time-outs, each ending its target early (artifacts under `run-<stamp>-b/artifacts/<target>/` on
the lab VM, not in the tree):

| Target | Stopped at | Kind |
|---|---|---|
| regex | 762 s | time-out (libFuzzer 11 s limit) |
| regex_dfa | 67 s | time-out and slow unit (11 s) |
| glob | 367 s | time-out |
| locale | 232 s | time-out |

Their status on `next` is in the re-check table below; to check again, use
`cargo xtask fuzz <target> --repro <artifact>` on the current `next`. No crash or sanitizer report was recorded.

Time-outs of the 2026-10-23 campaign, re-run on `next` (FUZZ-TIMEOUTS-RECHECK, 2026-11-04) with the campaign's limits
(`xtask/src/fuzz.rs` `settings`): `--repro` of each artifact.

| Target | Artifact | Limit | On `next` | Status |
|---|---|---|---|---|
| regex | `timeout-af9dfee4...` | 10 s | 9 ms | fixed (before this check; FUZZ-REGEX-TIMEOUTS) |
| regex_dfa | `timeout-d8d6be50...` | 10 s | 0.47 s | fixed (before this check; FUZZ-REGEX-TIMEOUTS) |
| regex_dfa | `slow-unit-d977c8d7...` | 10 s | 0.64 s | fixed (before this check; FUZZ-REGEX-TIMEOUTS) |
| locale | `timeout-bf6ed537...` | 20 s | still timed out before this change | fixed by this story: a composite conversion's padded field is bounded (`COMPOSITE_MAX`) and refused with `ENOMEM` before padding; test `composite_field_width_is_bounded_before_padding`; input pinned in `fuzz/seeds/locale` |
| glob | `timeout-5351ade2...` | 10 s | still times out | open: fix story proposed (cause and brief in the hand-back) |

The regex artifacts are pinned in `fuzz/seeds/regex`.
