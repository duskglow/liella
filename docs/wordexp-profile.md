# wordexp: profile and design note (X-5 `WORDEXP-PROFILE`, 2026-10-23)

Status: profile done, no code change. Measured on Haswell-EP host (Intel Xeon E5-2697 v3, Haswell-EP), head
`36168d3` (origin/next plus the new probe spec `validation/percall/wordexp.c`). `WRDE_NOCMD` throughout
(no shell runs). Counts are `instructions:u` per `wordexp` + `wordfree` call from `wl_percall`
(deterministic, sd 0.00); times are from `perf stat` and `cargo xtask bench` on a shared build host
(load 1.8 to 2.5), so they are indicative, not a booked timing run.

## 1. The register's wordexp ratios are stale

The saturation register (section 6) and `docs/benchmarks.md` give `wordexp_*` at t 2.3 to 4.2 and "about
4x glibc's instructions". Today, on the same CPU family:

| Row (`cbench_opt`) | Liella / glibc, time (median of 3 to 5 rounds) |
|---|---:|
| `wordexp_tilde_var` | 1.51 (worst 1.69) |
| `wordexp_arith` | 1.38 (worst 1.53) |
| `wordexp_quoted` | 1.73 (worst 1.76) |
| `wordexp_plain_8` | 0.61 (faster than glibc) |

Instructions per call (`wl_percall`, glibc dynamic 2.39, `en_US.UTF-8`):

| Case | glibc | Liella | Liella / glibc |
|---|---:|---:|---:|
| `we_one_word` (`abc`) | 1203 | 1071 | 0.89 |
| `we_plain_8` | 6425 | 3853 | 0.60 |
| `we_plain_32` | 29470 | 17084 | 0.58 |
| `we_long_word` (200 bytes) | 18493 | 5144 | 0.28 |
| `we_split_var` (`$WORDS`) | 3975 | 3934 | 0.99 |
| `we_param_forms` | 5558 | 6447 | 1.16 |
| `we_arith` | 3800 | 4457 | 1.17 |
| `we_arith_only` (`$((1+2))`) | 1944 | 2904 | 1.49 |
| `we_tilde_var` | 3215 | 4300 | 1.34 |
| `we_quoted` | 2860 | 3984 | 1.39 |
| `we_arith_long` (about 45 tokens) | 4388 | 8852 | 2.02 |
| `we_home_small` (`$HOME`) | 1910 | 2684 | 1.41 |
| `we_home_bigenv` (`$HOME`, 2,000 extra variables) | 13554 | 13684 | 1.01 |

Cycles and time per call (`perf stat`, 20,000 calls): `tilde_var` 1720 cycles / 575 ns against glibc 1115
cycles / 400 ns. Kernel instructions are equal in both libraries and `strace -c` shows no per-call
system call in either (the counts are the start-up ones), so the gap is user-mode only.

Two statements in `benchmarks/progress.md` no longer hold: "a fixed cost of about 1.3 us a call" (the
one-word call is 1071 instructions, below glibc's 1203) and "arithmetic operators found by string
comparison" (`arith::operator` is a `match` on bytes returning a code; the evaluator is precedence
climbing over codes). The 2.3 to 4.2 figures in `docs/benchmarks.md` come from an earlier head or a
different method; which one I cannot tell from the repository, and X-1's fresh bench will settle it.
I did not measure Zen 1 or Broadwell-EP.

## 2. Where Liella's time goes (flat `perf` profile, user symbols)

* `tilde_var`: the expander/driver `wordexp::<ProcessShell, Collected>` 31%; the boundary `wordexp`
  entry 8%; `Field::add` 7.8%; `Expander` methods about 12%; `Parser::words` 6.4%; `Words::push` 5.7%;
  environment lookup (`env_find`, `getenv`, `Shell::var`, `Shell::home`) about 9%; `calloc`/`free` 5%;
  `InlineVec::extend_from_slice` 3.3%. The heap is no longer the top item (33% in the 2026-10-03 profile).
* `plain_8`: the driver 34%, `Parser::words` 25%, `Words::push` 15%, the boundary entry 12%: the
  result vector and the one allocation per word, as glibc has.
* `arith_long`: `P::advance` 24%, `P::binary` 20%, `P::unary` 11%, `Parser::arith` 9%, `P::cond` 2.7%:
  the evaluator is about 58% of the call, and about 100 instructions per token more than glibc's whole
  call spends.
* Environment cost: `$HOME` with 2,000 extra variables costs 13.6k against 13.6k, about 5.5
  instructions per variable in both libraries, so Liella's lookup adds nothing that grows with the
  environment beyond glibc's own linear scan. `tilde_var` looks `HOME` up twice (the tilde and `$HOME`);
  each lookup is a scan.

## 3. Options

| Option | Gain (bounded by the profile) | Cost | Verdict |
|---|---|---|---|
| A. Rewrite: parse straight into output pieces, no fields | The driver, `Field`, `Words` and `Expander` are about 55% of `tilde_var`; at best that call goes from 1.34x to about 1.0x glibc's instructions, and `plain_8` is already below glibc | A rewrite of the 2,000-line expander (`crates/core/src/pattern/wordexp.rs`) | Reject: bounded gain of under 1,500 instructions on a call no measured workload makes hot |
| B. Cache the `HOME` lookup within one call | About 4% of `tilde_var` (one scan of about 9% total) | Small; must stay correct if `HOME` is changed by an assignment form such as `${HOME:=x}` | Candidate if a workload asks |
| C. Evaluator: avoid copying `Tok`, replace `level()` by a table indexed by code, cheaper `Result` plumbing in `binary`/`unary` | `arith_long` 2.0x towards about 1.5x; `we_arith` 1.17x already near parity | Medium; arithmetic semantics (overflow, shifts, short circuit) are covered by `wordexp_arith_lexer` | Candidate if a workload asks; long arithmetic words are rare |
| D. Cold costs: `first_wordexp` 6.7 us, `second_wordexp` 2.6 us in this bench | Larger than any steady-state gap for a short-lived process | Belongs to X-4 (`COLD-FIRST-CALLS`) | Hand to X-4 |

## 4. Recommendation

Close X-5 as a profile with no code story. The steady-state gap is now 0.3 to 2.0x in instructions and
0.6 to 1.7x in time, with the one-word fixed cost below glibc's, so the register row "no design for a
larger change is recorded" is answered: a larger change is not justified. Re-rank the row when X-1's
fresh bench lands; open options B and C only if the workload set (X-1's `-wlc`) shows `wordexp` in a
real program. Two checks before relying on this: the other bench hosts (Zen 1, Broadwell-EP) were not
run, and the 30-workload set was not searched for `wordexp` callers (no count run was in scope).

Reproduce: `bash validation/wl_percall.sh --base none --spec validation/percall/wordexp.c .` on a built
role; time rows: `cargo xtask bench --rows cbench_opt/wordexp_tilde_var,cbench_opt/wordexp_arith`.

## 5. PERF-WORDEXP follow-up (2026-10-23, Skylake-SP host, instructions:u per call, `wl_percall --base none`)

Two small changes (operator precedence stored by the lexer instead of classified twice; one scan in
`Field::add` for text with no pattern character). Before (section 1) -> after:

| Case | before | after | glibc | after / glibc |
|---|---:|---:|---:|---:|
| `we_arith_long` | 8852 | 8516 | 4379 | 1.94 |
| `we_tilde_var` | 4300 | 4227 | 3168 | 1.33 |
| `we_quoted` | 3984 | 3947 | 2828 | 1.40 |
| `we_arith_only` | 2904 | 2882 | 1935 | 1.49 |
| `we_arith` | 4457 | 4395 | 3773 | 1.16 |
| `we_plain_8` | 3853 | 3852 | 6209 | 0.62 |

Not at parity: `quoted`, `tilde_var`, `arith_only`, `home_small` stay 1.3 to 1.5x in instructions. The bound
is the structure named in option A (parse to segments, expand to pieces, assemble a field, emit): about
55% of `tilde_var` is that pipeline, and parity needs the rewrite of option A (est. 1.0 to 1.1x), which this
story does not do. Tried and left: caching the `HOME` lookup (option B, about 4%, correctness risk with
`${HOME:=x}`), evaluator table work beyond the precedence change (option C). Register row stays open for A.
