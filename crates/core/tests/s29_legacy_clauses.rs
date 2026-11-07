// Specified in part from spec §29 (phase 15: s29_3_c_0, s29_4_c_3 by LEGACY-STUBS; s29_2_c_0 by LEGACY-RPC-LIBNSL-PLACEHOLDER).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §29 (8 clauses).
//!
//! §29.4's malloc clauses (LG-11) run through the exports of `liella-boundary`; the STREAMS and
//! removed-syscall clauses (LEGACY-STUBS) run through `liella_boundary::posix::s29_legacy`; the
//! RPC/XDR clauses are pending; the NIS clause checks the `libnsl.so.1` placeholder (D-29.6).

#![allow(clippy::unwrap_used, missing_docs)]
#![no_builtins] // calls to libc names (memset, memcmp) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
// opaque-ok: these tests exercise the exported malloc/free family itself (LG-11); the optimizer must not drop the calls, and the blocks are checked through the boundary's memcmp

use liella_abi::ctypes::c_void;
use liella_boundary::posix::s03_memory as m;
use liella_boundary::posix::s04_strings as st;
use core::ptr::{null, null_mut};

use liella::error::Errno;
use liella_boundary::errno;
use liella_boundary::posix::s29_legacy as l;
use spectest::clause;

/// -1 with `errno` `e`.
fn fails(r: i32, e: Errno) -> bool {
    r == -1 && errno::get() == e.get()
}

clause!(s29_1_c_1, "29.1", "Contract", r##"**XDR** serializes C data to and from the External Data Representation (RFC 4506): encode, decode and free, for primitive types, arrays, strings, unions, pointers, and record-marking streams."##);
clause!(s29_1_c_2, "29.1", "Contract", r##"**The RPC runtime** provides client handles over UDP, TCP and Unix sockets, server dispatch, portmapper registration, and authentication flavours (`AUTH_NONE`, `AUTH_UNIX`/`AUTH_SYS`, `AUTH_DES`)."##);
/// §29.2 Contract: Client functions for Sun's Network Information Service (`yp_match`, `yp_first`, `yp_next`, `yp_all`, `yp_order`, `yp_master`, `yp_get_default_domain`, `yp_bind`, ...) and for NIS+.
/// D-29.6: `libnsl.so.1` is a placeholder whose every export fails at once as its row of
/// `crates/libc-so/placeholders/libnsl.tsv` says.
#[test]
fn s29_2_c_0() {
    nsl::check();
}

/// The `libnsl.so.1` placeholder (D-29.6), row by row against its failure table.
// Test-only helpers: a malformed table row or tool output is a test failure (panic), and the
// fields indexed are the ones the format guarantees.
#[allow(clippy::panic, clippy::indexing_slicing)]
mod nsl {
    use liella::error::Errno;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    /// The `errno` sentinel each call starts with: a stub that leaves `errno` alone keeps it.
    const SENTINEL: i32 = 4242;

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// The failure table: `(symbol, kind, value)`, the rows `cargo xtask so` builds the stubs from.
    fn table() -> Vec<(String, String, String)> {
        let text = std::fs::read_to_string(root().join("crates/libc-so/placeholders/libnsl.tsv")).unwrap();
        text.lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("symbol\t") && !l.is_empty())
            .map(|l| match l.split('\t').collect::<Vec<_>>()[..] {
                [n, k, v] => (n.to_string(), k.to_string(), v.to_string()),
                _ => panic!("failure table line {l:?}"),
            })
            .collect()
    }

    fn out(cmd: &mut Command) -> Output {
        cmd.output().unwrap()
    }

    fn stdout(cmd: &mut Command) -> String {
        let o = out(cmd);
        assert!(o.status.success(), "{cmd:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    /// What the driver prints for a row (see `driver`).
    fn expected(name: &str, kind: &str, value: &str) -> String {
        let s = SENTINEL;
        match kind {
            "int" => format!("{name} int {value} {s}"),
            "null" => format!("{name} null 1 {}", Errno::EOPNOTSUPP.get()),
            "result" => format!("{name} result {value} 1 {s}"),
            "void" => format!("{name} void {s}"),
            "free" => format!("{name} free {s}"),
            "text" => format!("{name} text {value} {s}"),
            "textbuf" => format!("{name} textbuf 1 {} 1 {s}", &value[..value.len().min(7)]),
            "perror" => format!("{name} perror {s}"),
            _ => panic!("unknown kind {kind} for {name}"),
        }
    }

    /// A C program over the product's generated bodies: one call per row, then the heap and
    /// the open descriptors.
    fn driver(rows: &[(String, String, String)], bodies: &Path) -> String {
        let mut c = format!("#include \"{}\"\n#include <fcntl.h>\n#include <malloc.h>\n", bodies.display());
        c.push_str("static void fds(const char *tag) {\n    printf(\"%s\", tag);\n    for (int fd = 0; fd < 1024; fd++)\n        if (fcntl(fd, F_GETFD) != -1)\n            printf(\" %d\", fd);\n    printf(\"\\n\");\n}\n");
        c.push_str("int main(void) {\n    char buf[8];\n    void *p;\n    fds(\"fds-before\");\n    /* stdio may give a stream its buffer at the first write: stderr's comes before the baseline. */\n    fputs(\"driver\\n\", stderr);\n");
        for (n, kind, _) in rows {
            let f = format!("liella_ph_{n}");
            // Twice: the second pass must leave the bytes in use as it found them (a lazy one-time
            // allocation of stdio in the first pass is not the stub's).
            c.push_str(&format!("    for (int pass = 0; pass < 2; pass++) {{\n    size_t h0 = mallinfo2().uordblks;\n    errno = {SENTINEL};\n"));
            c.push_str(&match kind.as_str() {
                "int" => format!("    {{ int r = {f}(); printf(\"{n} int %d %d\\n\", r, errno); }}\n"),
                "null" => format!("    p = {f}();\n    printf(\"{n} null %d %d\\n\", p == NULL, errno);\n"),
                "result" => format!("    p = {f}();\n    {{ int st = *(int *)p, e = errno, z = 1; for (size_t i = sizeof(int); i < 56; i++) z &= ((unsigned char *)p)[i] == 0; liella_ph_nis_freeresult(p); printf(\"{n} result %d %d %d\\n\", st, z, e); }}\n"),
                "void" => format!("    {f}();\n    printf(\"{n} void %d\\n\", errno);\n"),
                "free" => format!("    {f}(NULL);\n    {f}(calloc(1, 56));\n    printf(\"{n} free %d\\n\", errno);\n"),
                "text" => format!("    printf(\"{n} text %s %d\\n\", {f}(), errno);\n"),
                "textbuf" => format!("    memset(buf, 'x', sizeof buf);\n    {{ int same = {f}(41, \"label\", buf, sizeof buf) == buf; int none = {f}(41, \"label\", NULL, 8) == NULL; printf(\"{n} textbuf %d %s %d %d\\n\", same, buf, none, errno); }}\n"),
                "perror" => format!("    {f}(41, \"label\");\n    {f}(41, NULL);\n    printf(\"{n} perror %d\\n\", errno);\n"),
                _ => panic!("unknown kind {kind} for {n}"),
            });
            c.push_str(&format!("    size_t h1 = mallinfo2().uordblks;\n    if (pass == 1)\n        printf(\"heap {n} %zu %zu\\n\", h0, h1);\n    }}\n"));
        }
        c.push_str("    fds(\"fds-after\");\n    return 0;\n}\n");
        c
    }

    fn fd_set(text: &str, tag: &str) -> BTreeSet<i32> {
        let line = text.lines().find(|l| l.starts_with(tag)).unwrap_or_else(|| panic!("no {tag} line:\n{text}"));
        line.split_whitespace().skip(1).map(|f| f.parse().unwrap()).collect()
    }

    pub(super) fn check() {
        let root = root();
        let lib = root.join("target/liella");
        let so = lib.join("libnsl.so.1");
        let bodies = root.join("target/product/placeholders/libnsl.so.1.bodies.c");
        assert!(so.exists() && bodies.exists() && lib.join("libc.a").exists(), "the products are missing: run `cargo xtask so`");
        let rows = table();

        // 1. The table names exactly the libnsl.so.1 rows of versions.tsv (GLIBC_PRIVATE left out,
        //    R14-12), each at one version, all `compat`.
        let vt = std::fs::read_to_string(root.join("spec/abi/versions.tsv")).unwrap();
        let mut want: BTreeMap<String, String> = BTreeMap::new();
        for l in vt.lines() {
            let f: Vec<&str> = l.split('\t').collect();
            if f[0] == "libnsl.so.1" && f[2] != "GLIBC_PRIVATE" {
                assert_eq!(f[3], "compat", "only already-linked programs bind libnsl.so.1: {l}");
                assert!(want.insert(f[1].to_string(), f[2].to_string()).is_none(), "one version per name: {l}");
            }
        }
        let names: BTreeSet<&String> = rows.iter().map(|r| &r.0).collect();
        assert_eq!(names, want.keys().collect::<BTreeSet<_>>(), "the failure table against versions.tsv");

        // 2. The product defines each at its version as a compat (`@`) function, and nothing else.
        let dynsyms = stdout(Command::new("readelf").args(["--dyn-syms", "-W"]).arg(&so));
        let got: BTreeSet<String> = dynsyms.lines().filter_map(|l| match l.split_whitespace().collect::<Vec<_>>()[..] {
            [num, _, _, "FUNC", "GLOBAL", _, ndx, name, ..] if num.ends_with(':') && ndx != "UND" => Some(name.to_string()),
            _ => None,
        }).collect();
        assert_eq!(got, want.iter().map(|(n, v)| format!("{n}@{v}")).collect::<BTreeSet<_>>(), "libnsl.so.1's defined functions");
        let dynamic = stdout(Command::new("readelf").arg("-d").arg(&so));
        assert!(dynamic.contains("Library soname: [libnsl.so.1]") && dynamic.contains("Shared library: [libc.so.6]"), "{dynamic}");

        let dir = std::env::temp_dir().join(format!("liella-s29-2-nsl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let crt1 = lib.join("liella-crt1.o");
        let libc_so = lib.join("libc.so.6");

        // 3. A reference made the way an old program's is (at the version) links to every name;
        //    a plain reference does not (compat binding: no new program can link one).
        let mut old = String::new();
        let mut table_refs = Vec::new();
        for (i, (n, v)) in want.iter().enumerate() {
            old.push_str(&format!("extern void liella_ref_{i}(void);\n__asm__(\".symver liella_ref_{i}, {n}@{v}\");\n"));
            table_refs.push(format!("liella_ref_{i}"));
        }
        old.push_str(&format!("void (*const volatile liella_refs[])(void) = {{ {} }};\nint main(void) {{ return liella_refs[0] == 0; }}\n", table_refs.join(", ")));
        let (old_c, old_bin) = (dir.join("old.c"), dir.join("old"));
        std::fs::write(&old_c, old).unwrap();
        stdout(Command::new("cc").args(["-nostdlib", "-fno-builtin", "-o"]).arg(&old_bin).arg(&crt1).arg(&old_c).arg("-Wl,--no-as-needed").arg(&so).arg(&libc_so).arg("-lgcc"));
        let bound = stdout(Command::new("readelf").args(["--dyn-syms", "-W"]).arg(&old_bin));
        for (n, v) in &want {
            assert!(bound.lines().any(|l| l.contains(" UND ") && l.contains(&format!(" {n}@{v} "))), "the old-style program does not bind {n}@{v}:\n{bound}");
        }
        let new_c = dir.join("new.c");
        std::fs::write(&new_c, "extern int yp_match(void);\nint main(void) { return yp_match(); }\n").unwrap();
        let o = out(Command::new("cc").args(["-nostdlib", "-fno-builtin", "-o"]).arg(dir.join("new")).arg(&crt1).arg(&new_c).arg(&so).arg(&libc_so).arg("-lgcc"));
        assert!(!o.status.success() && String::from_utf8_lossy(&o.stderr).contains("yp_match"), "a new program linked compat-only yp_match: {o:?}");

        // 4. Every stub, through the product's generated bodies, in a single-threaded child: the
        //    row's result and errno, no descriptor opened, the heap as before.
        let (drv_c, drv) = (dir.join("drv.c"), dir.join("drv"));
        std::fs::write(&drv_c, driver(&rows, &bodies)).unwrap();
        stdout(Command::new("cc").args(["-static", "-nostdlib", "-fno-builtin", "-o"]).arg(&drv).arg(&crt1).arg(&drv_c).arg(format!("-L{}", lib.display())).arg(lib.join("libc.a")).arg("-lgcc"));
        let o = out(&mut Command::new(&drv));
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        assert!(o.status.success(), "the driver: {o:?}");
        let lines: BTreeSet<&str> = text.lines().collect();
        for (n, kind, value) in &rows {
            let e = expected(n, kind, value);
            assert!(lines.contains(e.as_str()), "{n} ({kind} {value}): expected `{e}`, the driver printed:\n{}", text.lines().filter(|l| l.starts_with(&format!("{n} "))).collect::<Vec<_>>().join("\n"));
        }
        let perror: String = std::iter::once("driver\n".to_string()).chain(rows.iter().filter(|r| r.1 == "perror").map(|r| format!("label: {}\n{}\n", r.2, r.2).repeat(2))).collect();
        assert_eq!(String::from_utf8_lossy(&o.stderr), perror, "the perror stubs' text on stderr");
        for (n, _, _) in &rows {
            let heap: Vec<&str> = text.lines().find(|l| l.starts_with(&format!("heap {n} "))).unwrap_or_else(|| panic!("no heap line for {n}")).split_whitespace().collect();
            assert_eq!(heap[2], heap[3], "{n}: bytes in use before and after its second call (a result must be freed)");
        }
        assert_eq!(fd_set(&text, "fds-before"), fd_set(&text, "fds-after"), "descriptors open before and after the stubs");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
#[test]
fn s29_3_c_0() {
    // As the glibc 2.39 probe (spec/behaviour/compat.md): the STREAMS calls fail with ENOSYS
    // whatever the arguments; isastream is 0 for an open descriptor (errno untouched) and EBADF
    // for any other.
    let f = std::fs::File::open("/dev/null").unwrap();
    let fd = std::os::fd::AsRawFd::as_raw_fd(&f);
    errno::set_raw(12345);
    assert_eq!((l::liella_compat_isastream(fd), errno::get()), (0, 12345), "an open descriptor");
    assert!(fails(l::liella_compat_isastream(-1), Errno::EBADF));
    assert!(fails(l::liella_compat_isastream(1 << 30), Errno::EBADF), "no such descriptor");
    let (mut flags, mut band) = (0, 0);
    for bad in [fd, -1] {
        assert!(fails(l::liella_compat_fattach(bad, c"/nonexistent".as_ptr()), Errno::ENOSYS));
        assert!(fails(l::liella_compat_getmsg(bad, null_mut(), null_mut(), &raw mut flags), Errno::ENOSYS));
        assert!(fails(l::liella_compat_putmsg(bad, null(), null(), 0), Errno::ENOSYS));
        assert!(fails(l::liella_compat_getpmsg(bad, null_mut(), null_mut(), &raw mut band, &raw mut flags), Errno::ENOSYS));
        assert!(fails(l::liella_compat_putpmsg(bad, null(), null(), 0, 0), Errno::ENOSYS));
    }
    assert!(fails(l::liella_compat_fattach(fd, null()), Errno::ENOSYS));
    assert!(fails(l::liella_compat_fdetach(c"/nonexistent".as_ptr()), Errno::ENOSYS));
    assert!(fails(l::liella_compat_fdetach(null()), Errno::ENOSYS));
}
clause!(s29_4_c_0, "29.4", "Contract", r##"Each keeps the contract it had when it was removed. Most are aliases of current functions, or stubs that fail with `ENOSYS`. Three are special:"##);
/// s29_4_c_1: `__libc_freeres()` releases libc's internal allocations so that leak checkers report
/// cleanly. Liella's `__libc_freeres` is inert (coordinator ruling #1317, differences row
/// `__libc_freeres`, compat flag COMPAT-11 registered): it releases nothing, so the call returns,
/// may be repeated, and leaves the library working (no state is torn down that a later call could
/// touch). A leak checker may report Liella's internal allocations as still reachable.
#[test]
fn s29_4_c_1() {
    let live = m::malloc(48);
    assert!(!live.is_null());
    st::memset(live, 0x33, 48);
    errno::set_raw(12345);
    m::__libc_freeres();
    m::__libc_freeres();
    assert_eq!(errno::get(), 12345, "an inert __libc_freeres leaves errno alone");
    // a block live across the call is untouched: nothing is released
    let want = [0x33u8; 48];
    assert_eq!(st::memcmp(live, want.as_ptr().cast::<c_void>(), 48), 0, "a live block survives __libc_freeres");
    let p = m::malloc(64);
    assert!(!p.is_null(), "the heap works after __libc_freeres");
    st::memset(p, 0x5a, 64);
    m::free(p);
    m::free(live);
    m::__libc_freeres();
    let q = m::__libc_malloc(8);
    assert!(!q.is_null());
    m::__libc_free(q);
}
/// s29_4_c_2: `__libc_malloc` and friends are aliases of `malloc` (the same heap and contracts)
/// that cannot be interposed: each is its own export that calls the heap directly, never through
/// the `malloc` name (`crates/boundary/src/posix/s03_memory.rs`), so a program's own `malloc`
/// does not reach them. Blocks cross freely between the two families.
#[test]
fn s29_4_c_2() {
    // __libc_malloc's block is an ordinary heap block: free() takes it.
    let p = m::__libc_malloc(40);
    assert!(!p.is_null());
    assert!(m::malloc_usable_size(p) >= 40);
    st::memset(p, 0x11, 40);
    m::free(p);
    // malloc's block goes back through __libc_free.
    let p = m::malloc(24);
    assert!(!p.is_null());
    m::__libc_free(p);
    // __libc_calloc zeroes; __libc_realloc keeps the contents.
    let zero = [0u8; 100];
    let p = m::__libc_calloc(10, 10);
    assert!(!p.is_null());
    assert_eq!(st::memcmp(p, zero.as_ptr().cast::<c_void>(), 100), 0, "__libc_calloc zeroes the block");
    st::memset(p, 0x7e, 100);
    let r = m::__libc_realloc(p, 4000);
    assert!(!r.is_null());
    let mark = [0x7eu8; 100];
    assert_eq!(st::memcmp(r, mark.as_ptr().cast::<c_void>(), 100), 0, "__libc_realloc keeps the contents");
    m::free(r);
    // The aligned forms: __libc_memalign, __libc_valloc and __libc_pvalloc (whole pages).
    let a = m::__libc_memalign(256, 10);
    assert!(!a.is_null() && a.addr() % 256 == 0);
    m::__libc_free(a);
    let v = m::__libc_valloc(10);
    assert!(!v.is_null() && v.addr() % 4096 == 0);
    m::free(v);
    let pv = m::__libc_pvalloc(10);
    assert!(!pv.is_null() && pv.addr() % 4096 == 0 && m::malloc_usable_size(pv) >= 4096);
    m::free(pv);
}
#[test]
fn s29_4_c_3() {
    // sysctl and __sysctl fail with ENOSYS and leave the buffers alone (the glibc 2.39 probe).
    let mut name = [1, 1];
    let mut buf = [7u8; 16];
    let mut len = buf.len();
    for call in [l::liella_compat_sysctl, l::liella_compat___sysctl] {
        assert!(fails(call(name.as_mut_ptr(), 2, buf.as_mut_ptr().cast(), &raw mut len, null_mut(), 0), Errno::ENOSYS));
        assert!(fails(call(null_mut(), 0, null_mut(), null_mut(), null_mut(), 0), Errno::ENOSYS));
    }
    assert_eq!((len, buf), (16, [7u8; 16]));
}
