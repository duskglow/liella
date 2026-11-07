# Plants: `cargo xtask plants`

A plant is a deliberate mutation of the code (or test) a story adds. The story's tests must catch it. `cargo xtask
plants` runs a list of plants in the way the project rules ask for, so a story does not need a hand-written plants
script (XTASK-PLANTS, plan/next-batch-1024-2.md).

**The tool is the required way to run plants (owner, 2026-11-05).** Do not write your own plants script: hand-written
scripts printed CAUGHT next to passing tests, hung for 150 minutes and silently skipped plants. The project's
per-plant limit is `--timeout 600` (the default); a plant whose command runs into it is CAUGHT ("caught by timeout").

```
cargo xtask plants <plants-file> --dir <new-directory> [--timeout <secs>]   # default and project limit: 600
```

Run it from the role tree on the build host, under plain `jobrun` (not `~/bin/gate`, which gives the job a private
`/dev/shm`). Use a directory that is NEW for every run and that is in RAM: `/dev/shm/claude/<story>/plants-<n>` on Zen 1 host,
`/mnt/zram/claude/<story>/plants-<n>` on Skylake-SP host (the RAM-disk host). Do not delete it: ram-sweep removes it later.

## What it does

1. It refuses the run (exit 3, nothing created) when `--dir` exists, when the plants file is malformed, or when any
   plant does not change exactly one site of its file in the current tree. It warns (`WARN-NOT-RAM`) when `--dir` is
   not on tmpfs or zram.
2. It copies the tree to `<dir>/src` with `rsync -a --exclude /target --exclude /fuzz/target` (the source only).
3. In the copy it runs `cargo xtask so` (the products), then each distinct plant command once on the unmutated copy.
   If one of them fails, it prints the last lines of its log and exits 2 before any plant is applied.
4. For each plant: it writes the mutated file (a fresh mtime, so cargo rebuilds), runs the command with `sh -c` in the
   copy (under `timeout`, default 600 s, the project's per-plant limit), then restores the file by writing it again (again a fresh mtime).
5. It prints one verdict per plant and a summary line. It exits 1 if any plant SURVIVED (or a step failed), else 0.

**Which commands rebuild the products** (XTASK-PLANTS-PRODUCTS-REBUILD). A test that links `libc.a` or
`liella-crt1.o` (a `*_products` test, a conformance program) sees a plant only if the products are rebuilt with
it, so for each plant whose command may use the products the tool runs `cargo xtask so` in the copy after
applying the plant and before the command (log `<id>.so.log`; if the mutant does not build, the plant is judged
on that log and the command is not run). A command is known not to use them only when it is one plain
`cargo test` whose targets are all named (`--lib`, `--bin <name>`, `--test <name>`) and no named test target
uses the products by the rule `cargo xtask affected` builds them for (its source or support files name
`target/liella`, `libc.a`, `liella-crt1` or `conformance`). Such a plant reuses the baseline build; anything
else (shell syntax, `cargo xtask affected`, all targets of a package, an unknown target or option) rebuilds.
After a plant that rebuilt the products, the next plant that does not use them first rebuilds them unmutated
(`<id>.so-restore.log`), so no command sees an earlier plant's mutant. The verdict line names the build
used (`build: the baseline products, not rebuilt`, or `rebuilt with the mutant in <n>s`).

Logs go to `<dir>/logs/` (`so.log`, `baseline-<n>.log`, `<id>.log`, `<id>.so.log`, `<id>.so-restore.log`). The tool writes nothing outside `--dir`.
`CARGO_TARGET_DIR` is removed from the commands' environment, so the copy builds into its own `target/`.

## The plants file

One plant per line. Blank lines and lines starting with `#` are skipped. Fields are separated by ` | ` (space, bar,
space):

```
id | path | pattern | cmd | msg:<regex>
id | path | pattern | cmd | equivalent:<reason>
id | path | pattern | cmd
```

- `id`: letters, digits, `-`, `_`, `.`; unique in the file. It names the log file.
- `path`: relative to the tree root, without `..`.
- `pattern`, one of:
  - `lit:<old> ==> <new>`: `<old>` must occur exactly once in the file (one line, no escapes). `lit:<old> ==>` deletes it.
  - `perl:s/<re>/<replacement>/<flags>`: one perl substitution (any delimiter). The same expression with `/g` must match
    exactly once. The `/g` and `/r` flags are refused. Use this form for multi-line patterns, or when the text contains
    ` | ` (write `\|` in the regex).
  - On a `.S` file, the pattern must name its function's label, for example
    `perl:s/(liella_machine_strchr:.*?)<pattern>/$1<replacement>/s`. An unanchored pattern mutates the first match in the
    file, which is often another function.
- `cmd`: the direct tests of the planted file, for example `cargo test -p liella --test stdio_short_io` (crates/core is package `liella`). For code
  reached only from C programs: `cargo xtask affected --only conformance:<prog> --with-exports --run`; a failed program's
  status and the last 40 lines of its stdout and stderr are on the console (D-X.27), so `msg:` can name the program's own
  failure message. A `cmd` may contain ` | `, but a job's output should not be piped.
- The last field:
  - `msg:<regex>` (a `grep -E` regular expression): the failing test or message the plant must produce. Name the
    failure, not just the test, because a passing run prints the test's name too (for example `msg:my_test.*FAILED|panicked`).
  - `equivalent:<reason>`: the mutant behaves like the original by design. If its command passes, it is EQUIVALENT.
  - none: the plant is judged by exit status alone.

## Verdicts

| Verdict | Meaning | Exit |
|---|---|---|
| `CAUGHT` | the command failed and its log matches `msg:` (or a plant declared equivalent was caught anyway), or it ran into the timeout | 0 |
| `WARN-NO-MSG` | the command failed, but its log does not match `msg:` | 0 |
| `WARN-RC-ONLY` | the command failed, and the plant has no `msg:` | 0 |
| `EQUIVALENT` | the command passed, and the plant is declared `equivalent:` | 0 |
| `SURVIVED` | the command passed on the mutant | 1 |

Every plant whose command failed (CAUGHT, WARN-NO-MSG, WARN-RC-ONLY) gets a second line,
`plants: <id>: caught by <test binary> <test name>`: the first failing test of its log (`cargo test`'s
`test <name> ... FAILED` under the binary of the last `Running` line, or nextest's `FAIL [..] <crate>::<target> <name>`).
Without a failing test line it names a panic's thread, a panic outside a named test with its binary, the binary cargo
asks to rerun, `the build (the mutant does not compile)`, or `no test the log names`; a timeout is `caught by timeout`.
EQUIVALENT and SURVIVED lines are unchanged (XTASK-PLANTS-CATCHER).

At the end the tool writes `<dir>/results.txt`: the plants file with a `# cargo xtask plants results (...)` comment
block, one line per plant (`# <id>: CAUGHT, caught by <binary> <test>`). Commit that file as
`handoff/<STORY>-plants.txt`, so which test caught each plant stays on record after the logs are gone; a later run from
it replaces the block.

List every WARN line in your hand-back. Exit codes: 0 all caught or equivalent; 1 a plant SURVIVED or a step failed;
2 the products or a baseline command failed; 3 refused input.

## Example

```
# STDIO-SHORT-IO plants
retry | crates/core/src/stdio/stream.rs | lit:while done < len { ==> if done < len { | cargo test -p liella --test stdio_short_io | msg:short_write.*(FAILED|panicked)
memchr-tail | csys/src/machine/memchr.S | perl:s/(liella_machine_memchr:.*?)jb\s+\.Ltail/$1jmp .Ltail/s | cargo test -p liella-boundary --test small_copy | msg:memchr
```

```
~/bin/jobrun short-io-plants plants -- sh -c '. ~/.cargo/env; cd ~/w/<role> && cargo xtask plants ~/work/<story>/plants.txt --dir /dev/shm/claude/<story>/plants-1'
```
