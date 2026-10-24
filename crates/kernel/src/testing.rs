//! Test support: [`Faulty`] wraps any [`Sys`](crate::sys::Sys) and makes chosen calls fail
//! (rule X5: library code is tested against `EINTR`, `ENOSPC`, `EMFILE`, `ENOMEM`, ... without
//! needing the kernel to produce them).
//!
//! A plan is a short list of `(method name, errno, skip, count)`: the named method succeeds
//! `skip` times, then fails with `errno` `count` times, then passes through again. Names are
//! the `Sys` method names (see [`crate::sys::FALLIBLE`]), and the few system calls that are not
//! `Sys` methods but whose callers consult a plan in tests ([`RAW_FALLIBLE`]).
//!
//! `Faulty` also implements [`Machine`](crate::machine::Machine) by delegation, so one value can
//! stand in for the whole kernel.

use core::cell::Cell;

use crate::errno::Errno;
use crate::machine::{Machine, X87Env};

/// Most faults one plan can hold.
pub const MAX_FAULTS: usize = 8;

/// System calls that are not `Sys` methods (they are `liella-boundary`'s `raw` calls, D-X.9) but
/// whose callers take a `Faulty` in tests and ask it with [`Faulty::take`] before the call: the
/// io_uring setup and probe of the AIO engine choice (D-11.11, through `selftest`).
pub const RAW_FALLIBLE: &[&str] = &["io_uring_setup", "io_uring_register"];

#[derive(Clone, Copy)]
struct Fault {
    name: &'static str,
    errno: Errno,
    skip: u32,
    count: u32,
}

/// A fault-injecting wrapper around a kernel implementation.
pub struct Faulty<S> {
    inner: S,
    plan: [Cell<Option<Fault>>; MAX_FAULTS],
    injected: Cell<u32>,
}

impl<S> Faulty<S> {
    /// Wrap `inner` with an empty plan (every call passes through).
    pub fn new(inner: S) -> Faulty<S> {
        Faulty { inner, plan: Default::default(), injected: Cell::new(0) }
    }

    /// Make `method` fail with `errno` `count` times after `skip` successful calls. Returns
    /// `false` if the plan is full or `method` is neither a fallible `Sys` method nor one of
    /// [`RAW_FALLIBLE`].
    pub fn fail(&self, method: &'static str, errno: Errno, skip: u32, count: u32) -> bool {
        if !crate::sys::FALLIBLE.contains(&method) && !RAW_FALLIBLE.contains(&method) {
            return false;
        }
        match self.plan.iter().find(|c| c.get().is_none()) {
            Some(slot) => {
                slot.set(Some(Fault { name: method, errno, skip, count }));
                true
            }
            None => false,
        }
    }

    /// Faults injected so far.
    pub fn injected(&self) -> u32 {
        self.injected.get()
    }

    /// The wrapped implementation.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Consume one planned fault for `method`, if one is due.
    pub fn take(&self, method: &str) -> Option<Errno> {
        for slot in &self.plan {
            let Some(mut f) = slot.get() else { continue };
            if f.name != method {
                continue;
            }
            if f.skip > 0 {
                f.skip = f.skip.saturating_sub(1);
                slot.set(Some(f));
                return None;
            }
            f.count = f.count.saturating_sub(1);
            slot.set(if f.count == 0 { None } else { Some(f) });
            self.injected.set(self.injected.get().saturating_add(1));
            return Some(f.errno);
        }
        None
    }
}

impl<S: Machine> Machine for Faulty<S> {
    fn thread_pointer(&self) -> usize { self.inner.thread_pointer() }
    fn cpuid(&self, leaf: u32, subleaf: u32) -> [u32; 4] { self.inner.cpuid(leaf, subleaf) }
    fn xcr0(&self) -> Option<u64> { self.inner.xcr0() }
    fn mxcsr(&self) -> u32 { self.inner.mxcsr() }
    fn set_mxcsr(&self, value: u32) { self.inner.set_mxcsr(value) }
    fn x87_control(&self) -> u16 { self.inner.x87_control() }
    fn set_x87_control(&self, value: u16) { self.inner.set_x87_control(value) }
    fn x87_status(&self) -> u16 { self.inner.x87_status() }
    fn x87_clear_exceptions(&self) { self.inner.x87_clear_exceptions() }
    fn x87_store_env(&self) -> X87Env { self.inner.x87_store_env() }
    fn x87_load_env(&self, env: &X87Env) { self.inner.x87_load_env(env) }
    fn pkru(&self) -> Option<u32> { self.inner.pkru() }
    fn set_pkru(&self, value: u32) -> bool { self.inner.set_pkru(value) }
    fn barrier(&self, bytes: &[u8]) { self.inner.barrier(bytes) }
}
