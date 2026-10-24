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

**Pending.** Both jobs were still running when the story was handed back. Copy the per-target rows from the two
`results.tsv` files into this table (corpus start -> end, libFuzzer `cov`/`ft` at the end, executions, finding):

| Target | Time | Corpus (start -> end) | cov / ft | Executions | Finding |
|---|---|---|---|---|---|
| (from `results.tsv`) | | | | | |

### Findings

Each finding (crash, hang/time-out, OOM) is a defect and gets its own fix story, not a rerun. Reproduce it with
`cargo xtask fuzz <target> --repro <artifact>` and minimise it with `--tmin`. A finding that may be a security weakness is not
described here (owner rule SECURITY-PRIVATE).

None recorded yet (campaign pending).
