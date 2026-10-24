//! Category *native* (`cargo xtask unsafe-audit`; this file only): the crate's declarations of
//! the native C ABI (`include/liella/native.h`, version node `LIELLA_NATIVE_1`), the
//! `GlobalAlloc` implementation over its heap functions, the byte views of an owned secure
//! block ([`SecureBlock`]), the [`entry!`](crate::entry)
//! export of a `no_main` program's C `main`, and, with feature `dylib`, the Rust ABI version's
//! exported string `LIELLA_RUST_ABI` and its note `.note.liella.rustabi`. Nothing else in the
//! crate may use `unsafe`.
//!
//! Rules (the boundary's S4, applied here):
//! * Every item is declared `safe`, so calls need no `unsafe` block.
//! * Items whose parameters are integers or Rust references (`&mut Timespec`) are sound for
//!   every argument by construction: a reference is a valid pointer to an object of the C
//!   structure's layout (asserted in `clock`, `fs` and `tz`), and the C function writes or
//!   reads exactly that object (layouts asserted in `clock`, `fs`, `tz` and `nscache`). They are `pub(crate)`.
//! * Items taking a pointer and a length are private and called only by the wrappers below,
//!   which pass a Rust slice's pointer and length, so no other code can pass an arbitrary
//!   address. The C side reads or writes only within `[ptr, ptr + len)` (the ABI's slice
//!   convention).
//! * The heap's items are private and called only by [`Allocator`], under `GlobalAlloc`'s
//!   contract, and by [`SecureBlock`], which owns one secure block and is the only code that
//!   views its bytes.
//!
//! `cargo xtask native-gen --check` compares every declaration here with
//! `spec/abi/native.tsv` (names, parameter names and C types), so they cannot drift from the
//! ABI.

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;

use crate::clock::Timespec;
use crate::fs::Stat;
use crate::tz::Tm;
use crate::nscache::Status as NscacheInfo;

// Soundness of the declarations: they match `include/liella/native.h` (checked against the
// table, above). Slices are passed as (pointer, length) of a live Rust slice, structures as
// references, and nothing the C library keeps refers to the caller's memory after the call
// (texts it stores, environment entries, are copies).
//
// UNSAFE(native): Rust cannot call the C library without declaring its functions.
#[allow(unsafe_code)]
unsafe extern "C" {
    safe fn liella_heap_alloc(size: usize, align: usize) -> *mut c_void;
    safe fn liella_heap_alloc_zeroed(size: usize, align: usize) -> *mut c_void;
    safe fn liella_heap_alloc_secure(size: usize, align: usize) -> *mut c_void;
    safe fn liella_heap_realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
    safe fn liella_heap_free(ptr: *mut c_void);
    /// Declared for completeness of the table check; no safe use (it takes any address).
    #[allow(dead_code)]
    safe fn liella_heap_usable_size(ptr: *const c_void) -> usize;
    safe fn liella_secure_zero(ptr: *mut c_void, len: usize) -> i64;

    safe fn liella_env_get(name: *const u8, name_len: usize, buf: *mut u8, buf_len: usize) -> i64;
    safe fn liella_env_set(name: *const u8, name_len: usize, value: *const u8, value_len: usize, overwrite: u32) -> i64;
    safe fn liella_env_unset(name: *const u8, name_len: usize) -> i64;
    pub(crate) safe fn liella_env_count() -> i64;
    safe fn liella_env_entry(index: usize, buf: *mut u8, buf_len: usize) -> i64;
    pub(crate) safe fn liella_environ_contract(on: i32) -> i32;
    pub(crate) safe fn liella_environ_reload();

    safe fn liella_fd_read(fd: i32, buf: *mut u8, len: usize) -> i64;
    safe fn liella_fd_write(fd: i32, buf: *const u8, len: usize) -> i64;
    safe fn liella_fd_pread(fd: i32, buf: *mut u8, len: usize, offset: i64) -> i64;
    safe fn liella_fd_pwrite(fd: i32, buf: *const u8, len: usize, offset: i64) -> i64;
    pub(crate) safe fn liella_fd_close(fd: i32) -> i64;
    pub(crate) safe fn liella_fd_seek(fd: i32, offset: i64, whence: u32) -> i64;
    pub(crate) safe fn liella_fd_stat(fd: i32, out: &mut Stat) -> i64;

    safe fn liella_fs_open(path: *const u8, path_len: usize, flags: u32, mode: u32) -> i64;
    safe fn liella_fs_stat(path: *const u8, path_len: usize, flags: u32, out: &mut Stat) -> i64;
    safe fn liella_fs_mkdir(path: *const u8, path_len: usize, mode: u32) -> i64;
    safe fn liella_fs_unlink(path: *const u8, path_len: usize) -> i64;
    safe fn liella_fs_rmdir(path: *const u8, path_len: usize) -> i64;
    safe fn liella_fs_rename(from: *const u8, from_len: usize, to: *const u8, to_len: usize) -> i64;

    pub(crate) safe fn liella_clock_realtime(out: &mut Timespec) -> i64;
    pub(crate) safe fn liella_clock_monotonic(out: &mut Timespec) -> i64;

    pub(crate) safe fn liella_tz_to_local(time: i64, out: &mut Tm) -> i64;
    pub(crate) safe fn liella_tz_from_local(tm: &mut Tm, time: &mut i64) -> i64;

    safe fn liella_locale_name(category: i32, buf: *mut u8, buf_len: usize) -> i64;
    safe fn liella_locale_decimal_point(buf: *mut u8, buf_len: usize) -> i64;

    pub(crate) safe fn liella_process_exit(status: i32) -> !;
    pub(crate) safe fn liella_process_arg_count() -> i64;
    safe fn liella_process_arg(index: usize, buf: *mut u8, buf_len: usize) -> i64;
    pub(crate) safe fn liella_process_id() -> i64;

    pub(crate) safe fn liella_nscache_enable(on: u32) -> i64;
    pub(crate) safe fn liella_nscache_category(category: u32, on: u32) -> i64;
    pub(crate) safe fn liella_nscache_invalidate(category: u32) -> i64;
    pub(crate) safe fn liella_nscache_status(out: &mut NscacheInfo) -> i64;

    pub(crate) safe fn liella_aio_use_threads() -> i64;

    pub(crate) safe fn liella_getpass_set_limit(bytes: usize) -> i64;

    pub(crate) safe fn liella_tz_invalidate() -> i64;
    pub(crate) safe fn liella_locale_invalidate() -> i64;
}

// ------------------------------------------------------------------------------ slices

pub(crate) fn env_get(name: &[u8], buf: &mut [u8]) -> i64 {
    liella_env_get(name.as_ptr(), name.len(), buf.as_mut_ptr(), buf.len())
}

pub(crate) fn env_set(name: &[u8], value: &[u8], overwrite: bool) -> i64 {
    liella_env_set(name.as_ptr(), name.len(), value.as_ptr(), value.len(), u32::from(overwrite))
}

pub(crate) fn env_unset(name: &[u8]) -> i64 {
    liella_env_unset(name.as_ptr(), name.len())
}

pub(crate) fn env_entry(index: usize, buf: &mut [u8]) -> i64 {
    liella_env_entry(index, buf.as_mut_ptr(), buf.len())
}

pub(crate) fn fd_read(fd: i32, buf: &mut [u8]) -> i64 {
    liella_fd_read(fd, buf.as_mut_ptr(), buf.len())
}

pub(crate) fn fd_write(fd: i32, buf: &[u8]) -> i64 {
    liella_fd_write(fd, buf.as_ptr(), buf.len())
}

pub(crate) fn fd_pread(fd: i32, buf: &mut [u8], offset: i64) -> i64 {
    liella_fd_pread(fd, buf.as_mut_ptr(), buf.len(), offset)
}

pub(crate) fn fd_pwrite(fd: i32, buf: &[u8], offset: i64) -> i64 {
    liella_fd_pwrite(fd, buf.as_ptr(), buf.len(), offset)
}

pub(crate) fn fs_open(path: &[u8], flags: u32, mode: u32) -> i64 {
    liella_fs_open(path.as_ptr(), path.len(), flags, mode)
}

pub(crate) fn fs_stat(path: &[u8], flags: u32, out: &mut Stat) -> i64 {
    liella_fs_stat(path.as_ptr(), path.len(), flags, out)
}

pub(crate) fn fs_mkdir(path: &[u8], mode: u32) -> i64 {
    liella_fs_mkdir(path.as_ptr(), path.len(), mode)
}

pub(crate) fn fs_unlink(path: &[u8]) -> i64 {
    liella_fs_unlink(path.as_ptr(), path.len())
}

pub(crate) fn fs_rmdir(path: &[u8]) -> i64 {
    liella_fs_rmdir(path.as_ptr(), path.len())
}

pub(crate) fn fs_rename(from: &[u8], to: &[u8]) -> i64 {
    liella_fs_rename(from.as_ptr(), from.len(), to.as_ptr(), to.len())
}

pub(crate) fn locale_name(category: i32, buf: &mut [u8]) -> i64 {
    liella_locale_name(category, buf.as_mut_ptr(), buf.len())
}

pub(crate) fn locale_decimal_point(buf: &mut [u8]) -> i64 {
    liella_locale_decimal_point(buf.as_mut_ptr(), buf.len())
}

pub(crate) fn process_arg(index: usize, buf: &mut [u8]) -> i64 {
    liella_process_arg(index, buf.as_mut_ptr(), buf.len())
}

pub(crate) fn secure_zero(buf: &mut [u8]) -> i64 {
    liella_secure_zero(buf.as_mut_ptr().cast(), buf.len())
}

// ------------------------------------------------------------------------------ heap

/// The alignment every heap block has at least (`LIELLA_HEAP_MIN_ALIGN`).
pub(crate) const HEAP_MIN_ALIGN: usize = 16;

/// Rust's global allocator on Liella's heap, the one behind `malloc`: blocks the program's
/// Rust code allocates come from the same heap as its C code's and the C library's own.
///
/// `realloc` resizes in place or moves through `liella_heap_realloc` when the alignment is at
/// most `LIELLA_HEAP_MIN_ALIGN` (16), which that function keeps; a block with a larger
/// alignment is moved by allocating, copying and freeing.
#[derive(Clone, Copy, Default, Debug)]
pub struct Allocator;

// SAFETY: every block comes from `liella_heap_alloc`/`_alloc_zeroed`/`_realloc`, which return
// a block of at least the size asked, aligned to the alignment asked (to at least 16 for
// `realloc`, used only for alignments up to 16), or NULL; such a block stays valid until it is
// passed to `liella_heap_free` or `liella_heap_realloc`, which `dealloc` and `realloc` do only
// with blocks this allocator returned (GlobalAlloc's contract on the caller). The heap is
// thread-safe (the C library's `malloc`), and no method unwinds.
// UNSAFE(native): `GlobalAlloc` is an unsafe trait; implementing it is the only way to make
// Liella's heap Rust's global allocator.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Allocator {
    // UNSAFE(native): a method of the trait above (its contract is the caller's).
    #[allow(unsafe_code)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        liella_heap_alloc(layout.size(), layout.align()).cast()
    }

    // UNSAFE(native): a method of the trait above.
    #[allow(unsafe_code)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        liella_heap_alloc_zeroed(layout.size(), layout.align()).cast()
    }

    // UNSAFE(native): a method of the trait above.
    #[allow(unsafe_code)]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        liella_heap_free(ptr.cast())
    }

    // UNSAFE(native): a method of the trait above.
    #[allow(unsafe_code)]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if layout.align() <= HEAP_MIN_ALIGN {
            return liella_heap_realloc(ptr.cast(), new_size).cast();
        }
        let fresh: *mut u8 = liella_heap_alloc(new_size, layout.align()).cast();
        if !fresh.is_null() {
            // UNSAFE(native) SAFETY: `ptr` is a live block of `layout.size()` bytes (the
            // caller's contract) and `fresh` a new, distinct block of `new_size` bytes; the
            // copy covers the smaller of the two, so both ranges are in bounds and disjoint.
            unsafe { core::ptr::copy_nonoverlapping(ptr, fresh, layout.size().min(new_size)) };
            liella_heap_free(ptr.cast());
        }
        fresh
    }
}

// ------------------------------------------------------------------------------ secure blocks

/// One secure heap block (`liella_heap_alloc_secure`, D-X.11), owned: its address (never 0)
/// and the length asked for. Dropping it releases the block through `liella_heap_free`,
/// which zeroes it; growing it goes through `liella_heap_realloc`, which keeps it secure and
/// zeroes the block it leaves. The address is kept as an integer, so the owner is `Send` and
/// `Sync` like a `Box<[u8]>`: it is the block's only owner, and `&self` gives only shared and
/// `&mut self` only exclusive views.
///
/// Every byte of the block's mapping is initialised: a secure block reads as zero when it is
/// allocated, `liella_heap_realloc` leaves the bytes past the old usable size zero, and bytes
/// are written only through [`SecureBlock::bytes_mut`].
#[derive(Debug)]
pub(crate) struct SecureBlock {
    addr: usize,
    len: usize,
}

impl SecureBlock {
    /// A new secure block of `len` bytes (at least 1, so that `realloc` never frees it), or
    /// `None` when the heap has none.
    pub(crate) fn new(len: usize) -> Option<SecureBlock> {
        let len = len.max(1);
        let p = liella_heap_alloc_secure(len, 1);
        (!p.is_null()).then(|| SecureBlock { addr: p.expose_provenance(), len })
    }

    /// The block's length.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Grow the block to `len` bytes (more than now), moving it if the heap must. False, with
    /// the block unchanged, when the heap has no room.
    pub(crate) fn grow(&mut self, len: usize) -> bool {
        if len <= self.len {
            return true;
        }
        let p = liella_heap_realloc(core::ptr::with_exposed_provenance_mut(self.addr), len);
        if p.is_null() {
            return false;
        }
        // Field by field: assigning a whole new `SecureBlock` would drop (free) this one.
        self.addr = p.expose_provenance();
        self.len = len;
        true
    }

    /// The block's bytes.
    // UNSAFE(native): the owned block becomes a slice; there is no other way to view memory
    // the C library handed out.
    #[allow(unsafe_code)]
    pub(crate) fn bytes(&self) -> &[u8] {
        // UNSAFE(native) SAFETY: `addr` is a live block of at least `len` initialised bytes
        // (the type's documentation) that only this owner reaches; the view borrows `self`.
        unsafe { core::slice::from_raw_parts(core::ptr::with_exposed_provenance(self.addr), self.len) }
    }

    /// The block's bytes, writable.
    // UNSAFE(native): as `bytes`.
    #[allow(unsafe_code)]
    pub(crate) fn bytes_mut(&mut self) -> &mut [u8] {
        // UNSAFE(native) SAFETY: as `bytes`, and the view borrows `self` exclusively.
        unsafe { core::slice::from_raw_parts_mut(core::ptr::with_exposed_provenance_mut(self.addr), self.len) }
    }
}

impl Drop for SecureBlock {
    fn drop(&mut self) {
        // The block came from `liella_heap_alloc_secure` or `liella_heap_realloc` and is
        // released once; the heap zeroes it before its memory can be reused.
        liella_heap_free(core::ptr::with_exposed_provenance_mut(self.addr));
    }
}

// ------------------------------------------------------------------------------ Rust ABI version

/// The Rust ABI version of this dylib as a NUL-terminated C string (`"lr1"`), exported
/// unmangled with feature `dylib` (D-X.10, docs/toolchain.md): `dlsym(handle,
/// "LIELLA_RUST_ABI")` on a loaded `libliella_rust.so.<abi>` names the version it is. Its
/// contents are built in `dylib`. In a process with several versions, a reference to the name
/// without a handle binds to the first library in lookup order, as for any symbol; the note
/// `.note.liella.rustabi` identifies each file.
#[cfg(feature = "dylib")]
// UNSAFE(native): an exported symbol must be unmangled; the name is Liella's own and defined
// nowhere else, and the object is immutable data.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub static LIELLA_RUST_ABI: [u8; crate::dylib::SYMBOL_LEN] = crate::dylib::symbol();

/// The ELF note `.note.liella.rustabi` (owner `Liella`, type 0x4c520001, descriptor the
/// identifier and the definition's hash) in its own section, which the linker puts in a
/// `PT_NOTE` segment. Kept by `#[used]`: nothing refers to it.
#[cfg(feature = "dylib")]
// UNSAFE(native): placing an object in a named section is an unsafe attribute; the section
// is Liella's own and holds nothing but this note, which is well formed and 4-aligned.
#[allow(unsafe_code)]
#[unsafe(link_section = ".note.liella.rustabi")]
#[used]
static RUST_ABI_NOTE: crate::dylib::Note<{ crate::dylib::DESC_PADDED }> = crate::dylib::note();

// ------------------------------------------------------------------------------ entry

/// Name a `#![no_main]` program's entry point: `liella_native::entry!(run);` defines the C
/// function `main`, which the start object (`liella-crt1.o`) calls through
/// `__libc_start_main`, as calling `run()`, a `fn() -> i32` whose result is the exit status
/// (`__libc_start_main` passes it to `exit`). The arguments are read with
/// [`process::args`](crate::process::args).
///
/// The export is the only unsafe attribute such a program needs. It is written here, in the
/// audited file (category *native*), so the program itself can forbid `unsafe_code`: the
/// compiler does not apply a crate's `unsafe_code` lint to another crate's macro expansion.
#[macro_export]
macro_rules! entry {
    ($run:path) => {
        /// The C entry point (`liella_native::entry!`).
        // UNSAFE(native): a program's `main` must be an unmangled C symbol.
        #[unsafe(no_mangle)]
        extern "C" fn main() -> i32 {
            $run()
        }
    };
}
