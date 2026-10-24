//! Process creation (spec §18.2; D-18.1, D-18.6): `fork` with `atfork` handlers and the
//! library's locks, `_Fork`, `clone` (`vfork` is csys/src/machine/vfork.S), and the handler registry.
//!
//! **`fork`** (D-18.6): the *prepare* handlers run newest first; the library's locks are taken
//! in one order (the handler registry, `popen`'s list and `system`'s count, the stdio list,
//! the thread list, the stack cache's, the setxid broadcast's, the time zone's, the heap's,
//! the environment's; none while the caller is the process's only thread, P5); then `clone`.
//! The parent releases them and runs
//! the *parent* handlers in registration order. The child releases them (it is their only
//! thread), resets the thread runtime to itself, drops the `SIGEV_THREAD` timers (timers are
//! not inherited, §23.4; D-23.3), parks the other threads' heap caches (their slabs stay
//! usable, M4), registers its robust list again, and runs the *child* handlers.

use core::sync::atomic::{AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::uapi::{CLONE_CHILD_CLEARTID, CLONE_CHILD_SETTID, CLONE_VFORK, CLONE_VM, SIGCHLD};
use liella_rt::futex::Scope;
use liella_rt::sync::RawMutex;

use crate::sys::CSys;
use crate::{ffi, heap, raw, tb, thread};

/// The registry's lock.
static ATFORK_LOCK: RawMutex = RawMutex::new();
/// The registered handlers, oldest first: heap nodes of four words (prepare, parent, child,
/// next).
static FIRST: AtomicUsize = AtomicUsize::new(0);
static LAST: AtomicUsize = AtomicUsize::new(0);

fn node(addr: usize) -> &'static [AtomicUsize] {
    tb::words64(addr, 4)
}

/// `__register_atfork(prepare, parent, child, dso)` / `pthread_atfork`: `ENOMEM` without
/// memory.
pub fn register(prepare: usize, parent: usize, child: usize) -> KResult<()> {
    let n = liella_legacy::malloc_compat::malloc(&heap::CTX, 32).map_err(|_| Errno::ENOMEM)?;
    let w = node(n);
    for (slot, v) in w.iter().zip([prepare, parent, child, 0]) {
        slot.store(v, Relaxed);
    }
    let _g = ATFORK_LOCK.lock(&CSys);
    match LAST.load(Relaxed) {
        0 => FIRST.store(n, Release),
        last => {
            if let Some(next) = node(last).get(3) {
                next.store(n, Release);
            }
        }
    }
    LAST.store(n, Release);
    Ok(())
}

fn call(f: usize) {
    if f != 0 {
        let _ = ffi::liella_machine_call0(f);
    }
}

/// The handlers of one kind (`which`: 0 prepare, 1 parent, 2 child), oldest first.
fn handlers(which: usize, mut each: impl FnMut(usize)) {
    let mut a = FIRST.load(Acquire);
    while a != 0 {
        let w = node(a);
        each(w.get(which).map_or(0, |x| x.load(Relaxed)));
        a = w.get(3).map_or(0, |x| x.load(Acquire));
    }
}

fn prepare_handlers() {
    // Newest first: collect into a bounded stack of addresses, then call backwards.
    let mut n = 0usize;
    handlers(0, |_| n = n.saturating_add(1));
    for i in (0..n).rev() {
        let mut k = 0usize;
        let mut f = 0usize;
        handlers(0, |h| {
            if k == i {
                f = h;
            }
            k = k.saturating_add(1);
        });
        call(f);
    }
}

fn lock_all() {
    ATFORK_LOCK.lock_raw(&CSys, Scope::Private);
    crate::spawn::fork_locks(true);
    liella_legacy::stdio_compat::hold_list(&CSys, true);
    thread::list_lock().lock_raw(&CSys, Scope::Private);
    thread::recycle_lock().lock_raw(&CSys, Scope::Private);
    crate::setxid::lock().lock_raw(&CSys, Scope::Private);
    liella_legacy::time_compat::lock().lock_raw(&CSys, Scope::Private);
    heap::DEBUG.lock().lock_raw(&CSys, Scope::Private);
    heap::HEAP.lock_all(&CSys);
    crate::process::hold_env_lock(true);
}

fn unlock_all() {
    crate::process::hold_env_lock(false);
    heap::HEAP.unlock_all(&CSys);
    heap::DEBUG.lock().unlock(&CSys, Scope::Private);
    liella_legacy::time_compat::lock().unlock(&CSys, Scope::Private);
    crate::setxid::lock().unlock(&CSys, Scope::Private);
    thread::recycle_lock().unlock(&CSys, Scope::Private);
    thread::list_lock().unlock(&CSys, Scope::Private);
    liella_legacy::stdio_compat::hold_list(&CSys, false);
    crate::spawn::fork_locks(false);
    ATFORK_LOCK.unlock(&CSys, Scope::Private);
}

/// `clone(SIGCHLD | CLONE_CHILD_SETTID | CLONE_CHILD_CLEARTID)` naming the thread block's TID
/// word, so the child's block holds its own TID (D-18.6).
fn raw_fork() -> KResult<i32> {
    let b = tb::current();
    let flags = u64::try_from(SIGCHLD).unwrap_or(17) | CLONE_CHILD_SETTID | CLONE_CHILD_CLEARTID;
    let tid = core::ptr::from_ref(&b.tid).expose_provenance();
    let tid = i64::from_ne_bytes(tid.to_ne_bytes());
    let r = raw::syscall(crate::nr::CLONE, [i64::from_ne_bytes(flags.to_ne_bytes()), 0, 0, tid, 0, 0])?;
    Ok(i32::try_from(r).unwrap_or(0))
}

fn child_reset() {
    thread::after_fork_child();
    crate::stacks::after_fork_child();
    crate::timer::after_fork_child();
    crate::mqueue::after_fork_child();
    crate::syslog::after_fork_child();
    crate::gai_async::after_fork_child();
    crate::aio::after_fork_child();
    crate::stdio::after_fork_in_child();
    crate::spawn::after_fork_child();
    liella_legacy::nscache::NSCACHE.after_fork_child();
    crate::data::__libc_single_threaded.store(1, Release);
}

/// `fork`. While the caller is the process's only thread (`thread::alone`, decided after the
/// *prepare* handlers, which may create one), no other thread can hold a library lock, so the
/// locks are neither taken nor released (P5): the child's state is the same, and neither side
/// writes the lock words while their pages are shared copy-on-write.
pub fn fork() -> KResult<i32> {
    prepare_handlers();
    let locks = !thread::alone();
    if locks {
        lock_all();
    }
    let r = raw_fork();
    if r == Ok(0) {
        if locks {
            unlock_all();
        }
        child_reset();
        heap::HEAP.park_others(&CSys, &heap::KernelPages);
        handlers(2, call);
    } else {
        if locks {
            unlock_all();
        }
        handlers(1, call);
    }
    r
}

/// `_Fork` (async-signal-safe): no handlers, no locks; the child's thread state is reset.
pub fn fork_raw() -> KResult<i32> {
    let r = raw_fork();
    if r == Ok(0) {
        child_reset();
    }
    r
}

/// `clone(fn, stack, flags, arg, ptid, tls, ctid)`: the child runs `fn(arg)` on `stack` and
/// exits with its result. `EINVAL` without a function or a stack.
pub fn clone(f: usize, stack: usize, flags: i32, arg: usize, ptid: usize, tls: usize, ctid: usize) -> KResult<i32> {
    if f == 0 || stack < 16 {
        return Err(Errno::EINVAL);
    }
    let sp = (stack & !15).wrapping_sub(16);
    // The two words the C layer pops in the child: the function and its argument (written
    // just below the caller's stack top, in its stack).
    crate::conv::store_words_user(sp, &[f, arg])?;
    let flags = u64::from(u32::from_ne_bytes(flags.to_ne_bytes()));
    // A child sharing our memory while we keep running: the mutexes must use atomics from
    // now on (MUTEX-3). With CLONE_VFORK the caller is suspended until the child executes or
    // exits, so the two never run at once.
    if flags & CLONE_VM != 0 && flags & CLONE_VFORK == 0 {
        thread::leave_lock_alone();
    }
    let r = ffi::liella_machine_clone_raw(flags, sp, ptid, ctid, tls);
    Errno::decode(r).map(|v| i32::try_from(v).unwrap_or(0))
}

/// `__register_atfork` (the DSO handle is for unregistering at `dlclose`, phase 14).
pub fn register_dso(prepare: usize, parent: usize, child: usize, _dso: usize) -> KResult<()> {
    register(prepare, parent, child)
}
