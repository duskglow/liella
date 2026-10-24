# Plants: `cargo xtask plants`

A plant is a deliberate mutation of the code (or test) a story adds. The story's tests must catch it. `cargo xtask
plants` runs a list of plants in the way the project rules ask for, so a story does not need a hand-written plants
script (XTASK-PLANTS, plan/next-batch-1024-2.md). For now the tool is advisory: hand-written scripts are still
accepted until the owner makes this the required path.

```
cargo xtask plants <plants-file> --dir <new-directory> [--timeout <secs>]
```

Run it from the role tree on the build host, under plain `jobrun` (not `~/bin/gate`, which gives the job a private
`/dev/shm`). Use a directory that is NEW for every run and that is in RAM: `/dev/shm/claude/<story>/plants-<n>` on keke,
`/mnt/zram/claude/<story>/plants-<n>` on sumire. Do not delete it: ram-sweep removes it later.

## What it does

1. It refuses the run (exit 3, nothing created) when `--dir` exists, when the plants file is malformed, or when any
   plant does not change exactly one site of its file in the current tree. It warns (`WARN-NOT-RAM`) when `--dir` is
   not on tmpfs or zram.
2. It copies the tree to `<dir>/src` with `rsync -a --exclude /target --exclude /fuzz/target` (the source only).
3. In the copy it runs `cargo xtask so` (the products), then each distinct plant command once on the unmutated copy.
   If one of them fails, it prints the last lines of its log and exits 2 before any plant is applied.
4. For each plant: it writes the mutated file (a fresh mtime, so cargo rebuilds), runs the command with `sh -c` in the
   copy (under `timeout`, default 1800 s), then restores the file by writing it again (again a fresh mtime).
5. It prints one verdict per plant and a summary line. It exits 1 if any plant SURVIVED (or a step failed), else 0.

Logs go to `<dir>/logs/` (`so.log`, `baseline-<n>.log`, `<id>.log`). The tool writes nothing outside `--dir`.
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
- `cmd`: the direct tests of the planted file, for example `cargo test -p liella-core --test stdio_short_io`. For code
  reached only from C programs: `cargo xtask affected --only conformance:<prog> --with-exports --run`. A `cmd` may contain
  ` | `, but a job's output should not be piped.
- The last field:
  - `msg:<regex>` (a `grep -E` regular expression): the failing test or message the plant must produce. Name the
    failure, not just the test, because a passing run prints the test's name too (for example `msg:my_test.*FAILED|panicked`).
  - `equivalent:<reason>`: the mutant behaves like the original by design. If its command passes, it is EQUIVALENT.
  - none: the plant is judged by exit status alone.

## Verdicts

| Verdict | Meaning | Exit |
|---|---|---|
| `CAUGHT` | the command failed and its log matches `msg:` (or a plant declared equivalent was caught anyway) | 0 |
| `WARN-NO-MSG` | the command failed, but its log does not match `msg:` | 0 |
| `WARN-RC-ONLY` | the command failed, and the plant has no `msg:` | 0 |
| `EQUIVALENT` | the command passed, and the plant is declared `equivalent:` | 0 |
| `SURVIVED` | the command passed on the mutant | 1 |

List every WARN line in your hand-back. Exit codes: 0 all caught or equivalent; 1 a plant SURVIVED or a step failed;
2 the products or a baseline command failed; 3 refused input.

## Example

```
# STDIO-SHORT-IO plants
retry | crates/core/src/stdio/stream.rs | lit:while done < len { ==> if done < len { | cargo test -p liella-core --test stdio_short_io | msg:short_write.*(FAILED|panicked)
memchr-tail | csys/src/machine/memchr.S | perl:s/(liella_machine_memchr:.*?)jb\s+\.Ltail/$1jmp .Ltail/s | cargo test -p liella-boundary --test small_copy | msg:memchr
```

```
~/bin/jobrun short-io-plants plants -- sh -c '. ~/.cargo/env; cd ~/w/<role> && cargo xtask plants ~/work/<story>/plants.txt --dir /dev/shm/claude/<story>/plants-1'
```
