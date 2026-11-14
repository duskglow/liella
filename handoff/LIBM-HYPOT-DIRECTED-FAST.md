# LIBM-HYPOT-DIRECTED-FAST: directed-mode hypot, envelope and FMA kernel (#4990)

State: branch `task/libm-hypot-directed-fast` (from origin/next 663210aaa, LIBM-HYPOT-DIRECTED landed), code head
187811b29; later commits are handoff/ only (HEAD in handoff/reports/LIBM-HYPOT-DIRECTED-FAST.json). Role mei:~/w/s2.

## Decisions in force

* records/pending/LIBM-HYPOT-DIRECTED-FAST-1.toml (D-NEW, series D-10, amends D-10.112: the coordinator assigns the
  id). D-10.104 (the kernel), D-10.112 (hardware first root), D-10.25 / D-10.50 (run-time FMA builds), D-10.13 /
  D-10.22 (the envelope, DAZ/FTZ honoured).

## Profile (step 1; mei, release products, `validation/wl_trace.sh --list`, call 11 of hypot in hypot_near_up)

Base next 663210aaa, 286 instructions per traced call (percall 296.6; glibc 68.8):

| part | instr |
|---|---|
| entry `hypot` (MXCSR read, default-word test) | 15 |
| `elem_switched` switch + `Entry` operand/result tests + black_box | ~54 (40 inline, 4 out-of-line MXCSR calls) |
| exit `leave` + `raise` (`enabled` read, `set` read-modify-write, trap tests) | ~56 |
| kernel `hypot_rounded` (baseline build, Dekker products) | ~162 |

After the envelope and FMA changes (head 1e62c7a74), 218 per traced call: entry 15, envelope 49 (incl. 2 MXCSR
writes of 3 instructions each), glue around the out-of-line FMA kernel (Option/Status return, dispatch byte) 43,
kernel (FMA build) 111. perf annotate was not run: the `--list` trace gives the exact per-instruction split on the
same products (perf sampling is lossy on these VMs, validation/wl_percall.sh header).

## Directions measured (percall, mei, instructions/call, base next 663210aaa, glibc dynamic)

| variant | head | near_up | near_down | near_zero | typ | near | bits |
|---|---|---|---|---|---|---|---|
| next (base) | 663210aaa | 296.6 | 296.6 | 296.6 | 105.0 | 105.0 | 63.1 |
| A: two-write directed envelope | acb35c909 | 236.6 | 236.6 | 236.6 | 105.0 | 105.0 | 63.1 |
| A + B: FMA build of `hypot_rounded` | 1e62c7a74 | 228.6 | 228.6 | 228.6 | 105.0 | 105.0 | 63.1 |
| glibc | | 68.8 | 68.8 | 68.8 | 65.9 | 68.8 | 41.5 |

Both kept (each won; no nearest row moved). Directed rows: 4.31x -> 3.32x glibc. Target "small factor" NOT reached.

* A (kept, -60): `elem_switched` takes `elem_rounding_switched` when the caller's word is default but for its
  rounding field: one MXCSR write to the default word, the computation, one write of `saved | flags`, `errno`.
  Same semantics as `Entry` + `exit` for that word (no DAZ/FTZ; all traps masked, so raising is setting). Applies
  to every `elem_fast` / `elem_fast_dir` function's directed path, not only hypot.
* B (kept, -8): `hypot_rounded` moved into the `kernels!` macro (FMA build `invtrig::fused::hypot_rounded`), called
  through `hot::hypot_rounded` (`dispatch::fma`). Smaller than expected: the products are only 2 of the kernel's
  work; the rest is the Ziv test, scaling, `parts`/`sq_cmp` setup and `hypot_finish`.
* C (kernel inlined into the directed path): NOT measured (budget). An FMA (`target_feature`) function cannot be
  inlined into the non-FMA envelope, so it needs the whole directed envelope as one FMA-build function selected at
  startup; the trace puts ~43 instructions in the glue it would remove.

## Open / next

* Next (follow-up story): (1) direction C as above (target the 43-instruction glue: `Option<(f64, Status)>` return
  through memory, the dispatch load); (2) the kernel's remaining 111: a directed-mode quick decision (the root plus
  one residual decides the directed rounding unless within ~2^-98 of a `double`, so `sq_cmp` and the midpoint logic
  are rare) - this is the structure change needed for a small factor; (3) adopting the cheaper envelope is already
  done for all `elem_fast` users; measure their directed rows (exp, log, pow) when convenient.
* Do not redo: the inline `ldmxcsr` (the static rule allows only the `stmxcsr` template in hot/fpregs.rs; the
  out-of-line write is 3 instructions), the A/B measurements above.
* Plan rows to read: D-10.104, D-10.112, D-10.25, D-10.50, D-10.13, D-10.22, the pending record.

## Results (labels are mei:~/logs/<label>.log)

All on code head 187811b29 unless noted.

* `libm-hypot-directed-fast-lint`: clippy --workspace --all-targets --all-features -D warnings, cargo xtask clippy,
  cargo xtask static, cargo xtask records check (1 pending): rc 0.
* `libm-hypot-directed-fast-gate` (affected --run --base origin/next): rc 0, 12 suites ok, 4541 passed / 0 failed
  (1173 ignored), conformance 8 / 0, peak 13G (page cache), oom_kill 0.
* `libm-hypot-directed-fast-plants` (handoff/LIBM-HYPOT-DIRECTED-FAST-plants.txt, products rebuilt per mutant):
  envelope-restore-default CAUGHT (s10_math_symbols hypot_on_the_hardware_root_...), fma-select-inverted CAUGHT
  (libm_fma_select libm_product_selects_the_fma_hypot_kernel_in_a_directed_mode); 0 SURVIVED, 0 WARN.
* `libm-hypot-directed-fast-sweeps` (gate's products): s10_math_symbols hypot 4 passed (entry in all four
  directions), math invtrig hypot 2 passed (kernel in every direction; entry in nearest and the three directed
  modes under both builds), elem_products 1 passed (C differential incl. hypot in all four directions),
  libm_fma_select hypot 1 passed (4000 upward calls: FMA 2882059, -FMA 3078058 instructions, same bits): rc 0.
* `libm-hypot-directed-fast-percall`: the table above, final row = 1e62c7a74's numbers again (228.6 directed;
  nearest 105.0 / 105.0 / 63.1 = next).
* Superseded (evidence for the table, older heads): `-percall0` (base), `-percallA` (acb35c909), `-percallB`
  (1e62c7a74), `-trace0`, `-traceB`.
