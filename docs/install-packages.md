# Installing the reference packages

Liella has reference Debian packages for Debian 13 (trixie) and Devuan 6 (excalibur), amd64.
They install Liella **beside** the system's C library: glibc (`libc6`, `libc6-dev`) and every
existing program are left as they are, and purging the packages leaves the system as it was.
Replacing `libc6` (Liella as the system's C library) is not supported yet.

## What they are

| Package | Like Debian's | Contents |
|---|---|---|
| `liella6` | `libc6` | the loader `ld-linux-x86-64.so.2` and the shared libraries (`libc.so.6`, `libm.so.6`, `libpthread.so.0`, `libdl.so.2`, `librt.so.1`, `libutil.so.1`, `libanl.so.1`, `libnsl.so.1`, `libc_malloc_debug.so.0`) |
| `liella6-dev` | `libc6-dev` | start files (`crt1.o`, `Scrt1.o`, `rcrt1.o`), the `libc.so` linker script, `libc_nonshared.a`, the static archives (`libc.a`, `libm.a`, and the empty `libpthread.a`, `libdl.a`, `librt.a`, `libutil.a`, `libanl.a`, `libresolv.a`), the `libm.so` link, and the `liella-gcc` / `liella-g++` compiler wrapper |

Everything is inside the sysroot `/usr/lib/liella`, laid out as Debian lays out `/` (merged
`/usr`, multiarch): the libraries in `/usr/lib/liella/usr/lib/x86_64-linux-gnu/`, the loader also
at `/usr/lib/liella/lib64/ld-linux-x86-64.so.2`. The only files outside it are
`/usr/bin/liella-gcc`, `/usr/bin/liella-g++`, their manual page and `/usr/share/doc/liella6*`.
The packages have no maintainer scripts: installing them does not touch `/etc/ld.so.conf`, the
system's loader cache or any other file.

Versions of snapshots are `0.0~snapshotYYYYMMDD+<commit>` (the commit's date and its first 12 hex
digits).

## Installing

```sh
sudo apt install ./liella6_<version>_amd64.deb ./liella6-dev_<version>_amd64.deb
```

`liella6-dev` depends on `gcc`, `binutils` and `libc6-dev`: Liella has no C header set of its own
yet, so programs compile with the system's headers (Liella implements the glibc 2.39 ABI those
headers describe). `g++` is recommended for `liella-g++`.

## Building a program against Liella

```sh
liella-gcc -O2 hello.c -o hello          # dynamic: runs on Liella's loader
liella-gcc -O2 -static hello.c -o hello  # static: Liella's libc.a
liella-g++ -O2 prog.cc -o prog           # C++: libstdc++ and the unwinder linked statically
liella-gcc -shared -fPIC plugin.c -o plugin.so
```

A program selects Liella through its program interpreter: `liella-gcc` links with
`--dynamic-linker=/usr/lib/liella/lib64/ld-linux-x86-64.so.2`, Liella's start files and Liella's
`libc.so`, so `readelf -l hello` shows that interpreter and the program never starts on the
system's loader. Liella's loader resolves its own libraries (`libc.so.6`, `libm.so.6`, ...) only in
`/usr/lib/liella/usr/lib/x86_64-linux-gnu`, and its default search for other libraries is that
directory too (its cache would be `/usr/lib/liella/etc/ld.so.cache`; none is installed). A program
that needs a system library (`-lz`, ...) names its directory, for example
`-Wl,-rpath,/usr/lib/x86_64-linux-gnu`; Liella's own libraries still come from the sysroot.
`-static-pie` and `-pg` are not supported by the wrapper. See `man liella-gcc`.

**Today (2026-11-13):** the loader does not yet search for a program's `DT_NEEDED` libraries at
start-up (it loads them only by path), so dynamically linked programs built with `liella-gcc`
stop with "the library search is not available in this build". Static programs run. Dynamic
programs run once the loader's start-time search lands; nothing in the packages changes then.

## Building the packages

```sh
tools/deb/build.sh            # from a git tree: exports HEAD, builds, writes target/deb/*.deb
tools/deb/check.sh target/deb # paths, no maintainer scripts, the wrapper's interpreter, static smoke
tools/deb/proof.sh target/deb trixie      # unprivileged chroot (mmdebstrap --mode=unshare)
tools/deb/proof.sh target/deb excalibur   # needs Devuan's keyring (KEYRING=...)
```

`build.sh` builds the release profile with the packages' loader layout (`LIELLA_PREFIX=/usr/lib/liella`,
`LIELLA_LIBDIR=usr/lib/x86_64-linux-gnu`); it needs `cargo`, `dpkg-deb` and `strip`. The build is
reproducible: building the same commit twice gives byte-identical packages (times from
`SOURCE_DATE_EPOCH`, root:root owners, fixed modes, sorted members). The design is the pending
decision `records/pending/RELEASE-PACKAGING-DEB-1.toml`.
