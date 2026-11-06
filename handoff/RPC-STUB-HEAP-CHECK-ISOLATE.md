# RPC-STUB-HEAP-CHECK-ISOLATE

State: branch task/rpc-stub-heap-check-isolate (cut from origin/next 6faf9548), head: see the report json.

## What changed
- `crates/boundary/tests/s29_rpc_clnt_svc_stubs.rs`: with `export` (the exports-interposed pass, `export,selftest`) the
  test binary's Rust allocations, libtest's main thread included, come from Liella's heap, so the process-wide
  `mallinfo2().uordblks` equality could see the harness's own allocations (+1024 / +176 bytes at a random stub).
  In that build the checks now run in a child forked (`liella_boundary::fork::fork`, as `regex_dfa_signal_fork` does)
  from the test's thread: the child's only thread, so no other thread can allocate and the equality stays an equality.
  The child catches a panic, writes `forked child: <message>` to the shared stderr (past libtest's capture) and exits
  1; the parent asserts `(pid, status 0)`. Without `export` the harness uses the host allocator, which never moves
  Liella's counters, so the checks run in place as before (no fork with host-allocator locks in play).
- Why fork and not the alternatives: a re-exec child (`s03_memory_symbols::isolated`) still runs libtest, whose main
  thread allocates (DL-BOOT-RELOC-5 saw 15/3900 with `--test-threads=1`); `harness = false` has no precedent here and
  nextest needs a libtest-compatible `--list`, and the trace/status tools read `#[test]`; `selftest::caller_blocks()`
  counts only the caller_alloc family, so it would miss an internal block a stub keeps (weaker).
- Optional item 4 done: `#[cfg_attr(not(feature = "runtime"), allow(dead_code))]` on `classed::start`,
  `nscache::start`, `sys::set_sysinfo_ehdr` (same form as ffi.rs).

## The same pattern elsewhere (item 2)
- `s29_rpc_auth_stubs`: no such test file on next (the brief's name); nothing to fix.
- `heap_threads.rs`: with `export` it already bounds the difference (`< 64 KiB` residue) instead of equality: safe.
- `s03_memory_symbols.rs` `sym_mallinfo2`: re-exec child plus a `NOISE` tolerance, never an equality: safe.
- `wordexp_reuse.rs`, `wordexp_block.rs`: `selftest::caller_blocks()` (per thread, exact): safe.
- `crates/core/tests/enomem_fs_walk.rs`: re-measures any window another thread's allocator call overlapped
  (`failinject::others_begun/others_done`): safe.
- `crates/core/tests/mallocdbg_products.rs`: compares C programs' output, not the test process heap: safe.

## Proof
- Repro loop (`~/work/RPC-STUB-HEAP-CHECK-ISOLATE/repro.sh` on the build host, written for this story since
  handoff/DL-BOOT-RELOC.md is not on this branch): builds the test in the export,selftest build in its own RAM target
  dir, runs the binary 3900 times (every second run `--test-threads=1`), concurrent with this story's gate and plants
  for load. Result: fails=0 of 3900, every run ran its test; load 4.0-4.4 on 48 threads (mei, job rpc-stub-heap-check-isolate-repro).
- Plants (`handoff/RPC-STUB-HEAP-CHECK-ISOLATE-plants.txt`, `cargo xtask plants`): svc_exit without its errno (export
  and host builds) and svc_exit keeping a 100-byte block (export build); the msg regexes require the forked child's
  message, so they prove the child's failures reach the parent. plants-2: 3/3 CAUGHT, no WARN (plants-1 superseded: the
  leak plant's unused allocation was optimised away; black_box keeps it).

## Next step
None beyond the jobs in the report. Do not redo: the item-2 survey above.
