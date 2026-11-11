//! DL-NONSHARED-OPTOUT prototype tests (`liella_legacy::owner_proto`, test-only under `selftest`):
//! attributing `atexit`/`at_quick_exit` registrations to a loaded module without the
//! `libc_nonshared.a` stub, and what an unload then runs or forgets. The cases mirror the
//! black-box probes recorded in handoff/DL-NONSHARED-OPTOUT.md (P2 cases 1-3) and the edge cases
//! of the brief. Addresses are model values; the recorder never calls them.

#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use liella_boundary::heap::KernelPages;
use liella_boundary::sys::CSys;
use liella_legacy::exit_handlers::{self, Registry, CXA};
use liella_legacy::obstack::CCall;
use liella_legacy::owner_proto::{attribute, finalize_module, opted_atexit, opted_quick, ModuleMap, Policy, PROCESS};

/// Records the handlers an exit-list run calls.
struct Ran(Vec<usize>);

impl CCall for Ran {
    fn call3(&mut self, f: usize, _a: usize, _b: usize, _c: usize) -> usize {
        self.0.push(f);
        0
    }
    fn call0(&mut self, f: usize) {
        self.0.push(f);
    }
    fn fatal(&mut self) -> ! {
        panic!("fatal")
    }
}

// Module A: text 0x1_0000..0x1_1000, data 0x1_2000..0x1_3000. Module B: text 0x2_0000..0x2_1000,
// data 0x2_2000..0x2_3000. The main program: 0x40_0000..0x40_8000.
const A: [(usize, usize); 2] = [(0x1_0000, 0x1_1000), (0x1_2000, 0x1_3000)];
const B: [(usize, usize); 2] = [(0x2_0000, 0x2_1000), (0x2_2000, 0x2_3000)];
const MAIN: [(usize, usize); 1] = [(0x40_0000, 0x40_8000)];
const IN_A: usize = 0x1_0400;
const IN_B: usize = 0x2_0400;
const B_HANDLER: usize = 0x2_0800;
const A_HANDLER: usize = 0x1_0800;
const B_DSO: usize = 0x2_2008; // B's `__dso_handle`, in its data

fn world() -> (ModuleMap<64>, u64, u64) {
    let mut m = ModuleMap::<64>::new();
    m.load(&MAIN).unwrap();
    let a = m.load(&A).unwrap();
    let b = m.load(&B).unwrap();
    (m, a, b)
}

#[test]
fn self_registration_matches_the_stub() {
    // Trace: no spec clause (DL-NONSHARED-OPTOUT prototype; probe P2 case 1: a module's own
    // atexit handler runs at its dlclose, and only there).
    let (m, a, b) = world();
    let (exit, quick) = (Registry::new(), Registry::new());
    assert!(exit.register(&CSys, &KernelPages, [CXA, 1, 0, B_DSO as u64]), "stub-style entry");
    assert!(exit.register(&CSys, &KernelPages, opted_atexit(&m, Policy::Caller, IN_B, B_HANDLER)));
    let mut ran = Ran(Vec::new());
    finalize_module(&exit, &quick, &CSys, &KernelPages, a, &A, true, &mut ran);
    assert_eq!(ran.0, [] as [usize; 0], "unloading A runs none of B's handlers");
    finalize_module(&exit, &quick, &CSys, &KernelPages, b, &B, false, &mut ran);
    assert_eq!(ran.0, [B_HANDLER, 1], "unloading B runs both, most recent first");
}

#[test]
fn helper_in_one_module_registering_anothers_handler() {
    // Trace: no spec clause (prototype; probe P2 case 2: with the stub, B's handler registered by
    // A's helper ran at A's dlclose).
    let (m, a, b) = world();
    assert_eq!(attribute(&m, Policy::Caller, 0, IN_A, B_HANDLER), a, "caller attribution names A, as the stub does");
    assert_eq!(attribute(&m, Policy::Handler, 0, IN_A, B_HANDLER), b, "handler attribution names B");
    let (exit, quick) = (Registry::new(), Registry::new());
    let mut ran = Ran(Vec::new());
    assert!(exit.register(&CSys, &KernelPages, opted_atexit(&m, Policy::Caller, IN_A, B_HANDLER)));
    finalize_module(&exit, &quick, &CSys, &KernelPages, a, &A, false, &mut ran);
    assert_eq!(ran.0, [B_HANDLER], "without the net: runs at A's unload, as the stub's probe did");
    assert!(exit.register(&CSys, &KernelPages, opted_atexit(&m, Policy::Caller, IN_A, B_HANDLER)));
    finalize_module(&exit, &quick, &CSys, &KernelPages, b, &B, true, &mut ran);
    assert_eq!(ran.0, [B_HANDLER, B_HANDLER], "with the net: B unloading first runs it before its code goes");
    finalize_module(&exit, &quick, &CSys, &KernelPages, a, &A, true, &mut ran);
    assert_eq!(ran.0.len(), 2, "and A's unload does not run it again");
}

#[test]
fn tail_call_names_the_callers_caller_and_the_net_covers_it() {
    // Trace: no spec clause (prototype; edge case: A's `return atexit(a_cleanup)` compiled as a
    // tail call returns straight to its caller in B).
    let (m, a, b) = world();
    let (exit, quick) = (Registry::new(), Registry::new());
    let e = opted_atexit(&m, Policy::Caller, IN_B, A_HANDLER);
    assert_eq!(e[3], b, "the return address is B's");
    assert!(exit.register(&CSys, &KernelPages, e));
    let mut ran = Ran(Vec::new());
    finalize_module(&exit, &quick, &CSys, &KernelPages, a, &A, false, &mut ran);
    assert_eq!(exit.live(&CSys, &KernelPages), 1, "caller attribution alone leaves A's handler registered after A is gone");
    finalize_module(&exit, &quick, &CSys, &KernelPages, a, &A, true, &mut ran);
    assert_eq!(ran.0, [A_HANDLER], "the handler-address net runs it at A's unload");
    assert_eq!(exit.live(&CSys, &KernelPages), 0);
}

#[test]
fn jit_and_anonymous_memory_belong_to_the_process() {
    // Trace: no spec clause (prototype; edge case: call site and handler in no module).
    let (mut m, _, _) = world();
    const JIT: usize = 0x7f00_0000_1000;
    for p in [Policy::Caller, Policy::Handler] {
        assert_eq!(attribute(&m, p, 0, JIT, JIT + 0x40), PROCESS);
    }
    let (exit, quick) = (Registry::new(), Registry::new());
    assert!(exit.register(&CSys, &KernelPages, opted_atexit(&m, Policy::Caller, JIT, JIT + 0x40)));
    // A module mapped later where the JIT code was (the JIT region freed first): the owner was
    // fixed at registration, so its unload does not take the entry by owner.
    let late_segs = [(0x7f00_0000_0000, 0x7f00_0001_0000)];
    let late = m.load(&late_segs).unwrap();
    let mut ran = Ran(Vec::new());
    finalize_module(&exit, &quick, &CSys, &KernelPages, late, &late_segs, false, &mut ran);
    assert_eq!(ran.0, [] as [usize; 0], "an unrelated unload does not take a process entry");
    exit_handlers::run_all(&exit, &CSys, &KernelPages, 0, &mut ran);
    assert_eq!(ran.0, [JIT + 0x40], "it runs at exit");
}

#[test]
fn reload_at_the_same_address_is_a_new_owner() {
    // Trace: no spec clause (prototype; edge case: a library dlopen'd, closed and opened again).
    let (mut m, _, b) = world();
    assert_eq!(m.load(&B), None, "a second open does not map B again: overlapping ranges are refused");
    let (exit, quick) = (Registry::new(), Registry::new());
    assert!(exit.register(&CSys, &KernelPages, opted_atexit(&m, Policy::Caller, IN_B, B_HANDLER)));
    let mut ran = Ran(Vec::new());
    finalize_module(&exit, &quick, &CSys, &KernelPages, b, &B, true, &mut ran);
    m.unload(b);
    assert_eq!(m.owner_of(IN_B).0, None, "unloaded");
    let b2 = m.load(&B).unwrap();
    assert_ne!(b2, b, "a new load gets a new handle");
    assert!(exit.register(&CSys, &KernelPages, opted_atexit(&m, Policy::Caller, IN_B, B_HANDLER + 1)));
    assert!(exit.register(&CSys, &KernelPages, [CXA, 7, 0, b]), "an entry tagged with the old handle");
    finalize_module(&exit, &quick, &CSys, &KernelPages, b2, &B, false, &mut ran);
    assert_eq!(ran.0, [B_HANDLER, B_HANDLER + 1], "the new instance's unload takes only its own entry");
    assert_eq!(exit.live(&CSys, &KernelPages), 1);
}

#[test]
fn explicit_dso_from_a_stub_or_foreign_module_wins() {
    // Trace: no spec clause (prototype; edge case: modules built with the stub, including ones
    // linked against another C library's archive, pass their own handle).
    let (m, a, _) = world();
    assert_eq!(attribute(&m, Policy::Caller, B_DSO, IN_A, A_HANDLER), B_DSO as u64);
    let (exit, quick) = (Registry::new(), Registry::new());
    assert!(exit.register(&CSys, &KernelPages, [CXA, 9, 0, B_DSO as u64]));
    let mut ran = Ran(Vec::new());
    finalize_module(&exit, &quick, &CSys, &KernelPages, a, &A, true, &mut ran);
    assert_eq!(ran.0, [] as [usize; 0], "A's unload leaves B's stub entry alone");
    finalize_module(&exit, &quick, &CSys, &KernelPages, 0xdead, &B, false, &mut ran);
    assert_eq!(ran.0, [9], "a `dso` inside B's ranges is B's");
}

#[test]
fn quick_exit_entries_are_forgotten_on_unload() {
    // Trace: no spec clause (prototype; probe P2 case 3: after dlclose, the module's
    // at_quick_exit handler did not run at quick_exit).
    let (m, a, b) = world();
    let (exit, quick) = (Registry::new(), Registry::new());
    assert!(quick.register(&CSys, &KernelPages, opted_quick(&m, Policy::Caller, IN_B, B_HANDLER)));
    assert!(quick.register(&CSys, &KernelPages, opted_quick(&m, Policy::Caller, IN_A, A_HANDLER)));
    let mut ran = Ran(Vec::new());
    finalize_module(&exit, &quick, &CSys, &KernelPages, b, &B, false, &mut ran);
    assert_eq!(quick.live(&CSys, &KernelPages), 1, "B's quick-exit entry is gone, A's stays");
    exit_handlers::run_all(&quick, &CSys, &KernelPages, 0, &mut ran);
    assert_eq!(ran.0, [A_HANDLER], "quick_exit runs only A's");
    let _ = a;
}

#[test]
fn lookup_is_logarithmic_and_exact() {
    // Trace: no spec clause (prototype; the per-registration cost of eager caller attribution).
    let mut m = ModuleMap::<512>::new();
    let mut owners = Vec::new();
    // Loaded out of address order, as dlopen would: 256 modules of two ranges each.
    for i in (0..256usize).map(|i| (i * 97) % 256) {
        let base = 0x1000_0000 + i * 0x10_0000;
        owners.push((base, m.load(&[(base, base + 0x8000), (base + 0x9000, base + 0xc000)]).unwrap()));
    }
    let mut worst = 0;
    for &(base, h) in &owners {
        for (addr, want) in [(base, Some(h)), (base + 0x7fff, Some(h)), (base + 0x8000, None), (base + 0x9000, Some(h)), (base + 0xc000, None)] {
            let (got, steps) = m.owner_of(addr);
            assert_eq!(got, want, "address {addr:#x}");
            worst = worst.max(steps);
        }
    }
    assert!(worst <= 10, "512 ranges: at most 10 comparisons, saw {worst}");
    assert_eq!(m.ranges_of(owners[3].1).count(), 2);
}
