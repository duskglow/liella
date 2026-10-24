# Binary-compatibility sweep (plan phase 15, LG-0c `BINCOMPAT-SWEEP-STATIC`)

`validation/bincompat_sweep.py` reads every regular x86_64 ELF under `/usr/bin`, `/usr/sbin` and
`/usr/lib*` (symlinks not followed) with `readelf -W -l -d -V --dyn-syms`, and compares each
version need on a glibc ABI library (the `library` column of `spec/abi/versions.tsv`) with the
manifest (default / compat / private / absent) and with the exports of Liella's built products
(`target/liella/*.so*`). The script's docstring gives the rules; `--self-test` checks them on fixed
readelf texts (8 plants, all caught, job `bincompat-sweep-static-plants*` on kanon).

Rerun (a later story, e.g. after DL-14 or LG-1), from a role tree after `cargo xtask so`:

    python3 -I validation/bincompat_sweep.py --out <dir>
    # another host, no build: reuse a product export set
    python3 -I validation/bincompat_sweep.py --liella-exports <dir>/liella-exports.tsv --out <dir2>

Two runs on one host diff clean (except `run.txt`); every output is sorted.

## Files (one directory per host)

| file | content |
|---|---|
| `summary.md` | counts, top compat needs, top blockers |
| `compat-needs.tsv` | every needed compat `sym@VER`, ranked by ELF users |
| `missing-needs.tsv` | every needed `sym@VER` Liella does not export yet, ranked |
| `version-nodes.tsv` | every needed (library, version node) and whether Liella defines it |
| `programs.tsv` | every program (ELF with PT_INTERP): direct / transitive status, first blocker |
| `glibc-components.txt` | ELFs counted apart as glibc's own files (glibc soname or a GLIBC_PRIVATE need) |
| `liella-exports.tsv` | (kanon) the export set of the products used; shiki's run used this file |
| `run.txt` | host, date, host glibc, product hashes |

## Results (2026-10-23, Liella products of `task/bincompat-sweep-static` = origin/next `01a1d6d0` + this script)

Both hosts: Ubuntu 24.04.5, glibc 2.39. Products: `libc.so.6=0f78e1cfca5abdec`, `libm.so.6=8dd3e056a257ebbf`,
`libpthread.so.0=9fa91a1dcba1b2f7` (sha256 prefixes, kanon `run.txt`).

| | kanon | shiki |
|---|---|---|
| ELFs / dynamic (non-glibc) / programs | 2811 / 1930 / 1036 | 2838 / 1957 / 1059 |
| glibc components counted apart | 259 | 259 |
| distinct glibc `sym@VER` needed | 1358, all default | 1365, all default |
| compat `sym@VER` needed | **0** | **0** |
| needed `sym@VER` Liella lacks | 17 | 17 (the same list) |
| programs ok directly | 973 | 993 |
| programs ok with their libraries | 483 | 486 |

Programs present on both hosts have the same status on both (checked with `join` over `programs.tsv`).

1. **No real binary on these hosts binds a compat version.** Every compat `sym@VER` that appears is
   bound only by glibc's own files (RPC names such as `clnt_create@GLIBC_2.2.5`,
   `__malloc_initialize_hook@GLIBC_2.2.5`; seen in a first run before these files were counted
   apart), which Liella replaces. So host binaries do not rank the LG compat stories; that ranking has to come from
   the corpus of LG-19/LG-22 (test-built `.symver` programs, R15-8) and from LG-0a's plan table.
2. **The 17 missing names are defaults**, all phase-14 or other phase-15 work: `dlopen`, `dlsym`,
   `dlclose`, `dlerror`, `dladdr`, `dlvsym`, `dlinfo@GLIBC_2.34`, `_dl_mcount_wrapper_check`;
   `ld-linux-x86-64.so.2`'s `__tls_get_addr@GLIBC_2.3` and `__libc_stack_end` (no Liella ld.so yet);
   profiling `monstartup`, `_mcleanup`, `__profile_frequency` (§28); `libresolv.so.2`'s `__b64_ntop`,
   `__b64_pton`, `ns_initparse`, `ns_parserr` (Liella builds `libresolv.a` only).
3. **Version node `GLIBC_ABI_DT_RELR`** is needed in `libc.so.6` by 26 ELFs (8 programs blocked by it
   directly); Liella's `libc.so.6` does not define it and `versions.tsv` has no row for it (the
   manifest holds symbol rows only). The node tells the dynamic linker that DT_RELR relocations
   are understood, so it belongs with the ld.so story that implements DT_RELR (`plan/ph14/design.md`
   mentions DT_RELR).
4. **Transitive blockers**: most programs that are ok directly fail through a library that needs
   `ld-linux-x86-64.so.2` (`libsystemd-shared`, `libselinux`, `libuuid`: `__tls_get_addr`) or
   `dlerror` (`libcrypto.so.3`). These go away with DL-14's ld.so and the `dl*` exports.
5. **LG-22's first corpus**: the programs with `transitive` = `ok` in `programs.tsv` (483 on kanon).

Not judged (stated in the script): unversioned references (2 ELFs on kanon, 3 on shiki), the
program interpreter itself, `dlopen`ed plugins, and library lookup beyond RUNPATH/`$ORIGIN` and
the standard directories (no `ld.so.cache`).
