//! Runtime mechanism tests (implementation-plan.md §4.2, phases 3, 4 and 6).
//!
//! Hand-written, not generated: these verify internal primitives IP-1..IP-12 (plan §5), which
//! have no spec clause of their own. Each test names the primitive and the spec clauses whose
//! guarantees depend on it, for example `ip3_rawmutex_zero_init_is_unlocked` (spec §22.4 static
//! initializer, decision D-22.1).
