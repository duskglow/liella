/* liella C syscall layer: public declarations (see csys/README.md).
 *
 * Naming:
 *   long     liella_sys_<syscall>(...)   class sys:     raw kernel result (value or -errno)
 *   ...      liella_machine_<primitive>(...) class machine: assembly primitives (src/machine, assembly)
 *   ...      liella_va_<type>(va_list *)  class adapter: va_arg accessors
 *
 * Declarations are added phase by phase (implementation-plan.md §6). The Rust declarations in
 * crates/boundary/src/ffi.rs must match this header exactly. */
#ifndef LIELLA_CSYS_H
#define LIELLA_CSYS_H

#include <stddef.h>
#include <stdint.h>

#define LIELLA_CSYS_ABI_VERSION 1u

/* Phase 0: proves the layer builds, links and is reachable from Rust. */
uint32_t liella_csys_abi_version(void);

/* Class machine (src/machine, assembly; owner decisions: approved set plus OD-C1). */
uintptr_t liella_machine_tp(void);
uint64_t liella_machine_rdtsc(void);
void liella_machine_cpuid(uint32_t leaf, uint32_t subleaf, uint32_t out[4]);
uint64_t liella_machine_xgetbv0(void);
uint32_t liella_machine_mxcsr_get(void);
void liella_machine_mxcsr_set(uint32_t value);
uint32_t liella_machine_mxcsr_mask(void);
uint32_t liella_machine_x87_cw_get(void);
void liella_machine_x87_cw_set(uint32_t value);
uint32_t liella_machine_x87_sw_get(void);
void liella_machine_x87_clex(void);
void liella_machine_x87_env_store(void *env28);
void liella_machine_x87_env_load(const void *env28);
double liella_machine_sqrtsd(double x);
float liella_machine_sqrtss(float x);
void liella_machine_fsqrt(const void *x10, void *out10);
uint32_t liella_machine_rdpkru(void);
void liella_machine_wrpkru(uint32_t value);
void liella_machine_barrier(const void *p);
void liella_machine_probe_addrs(uintptr_t out[3]);
void liella_machine_probe_run(uint32_t mcount, uint32_t n);
long liella_machine_call0(uintptr_t fn);
int liella_machine_once(uintptr_t once, uintptr_t init);
long liella_machine_guard(uintptr_t body, uintptr_t ctx, uintptr_t undo);
long liella_machine_call3(uintptr_t fn, long a, long b, long c);
long liella_machine_call4(uintptr_t fn, long a, long b, long c, long d);
long liella_machine_vcall2(uintptr_t fn, long a, void *b);
long liella_machine_vcall3(uintptr_t fn, void *a, void *b, void *c);
void liella_machine_restorer(void);
/* Scanners and ERMS copies (OD-C2, rule P7, D-4.14; src/machine/scan.S). */
void liella_machine_set_scan_level(uint32_t level);
uint32_t liella_machine_scan_level_get(void);
size_t liella_machine_strlen(const char *s);
size_t liella_machine_strnlen(const char *s, size_t max);
/* The direct strcpy entry (STRCPY-SHORT-2; machine/scan.S). */
char *liella_machine_strcpy(char *d, const char *s);
/* The direct strncpy entry (STRNCPY-SHORT; machine/scan.S). */
char *liella_machine_strncpy(char *d, const char *s, size_t n);
/* The direct __strncpy_chk and stpncpy entries (STRNCPY-FAMILY; machine/scan.S). */
char *liella_machine_strncpy_chk(char *d, const char *s, size_t n, size_t destlen);
char *liella_machine_stpncpy(char *d, const char *s, size_t n);
/* The direct __stpncpy_chk entry (STPNCPY-ALIAS; machine/scan.S). */
char *liella_machine_stpncpy_chk(char *d, const char *s, size_t n, size_t destlen);
char *liella_machine_stpcpy(char *d, const char *s);
char *liella_machine_stpcpy_chk(char *d, const char *s, size_t room);
char *liella_machine_strcpy_chk(char *d, const char *s, size_t room);
const void *liella_machine_rawmemchr(const void *s, int c);
/* The direct rawmemchr entry (SCAN-TAIL): the NULL test, then the kernel above. */
const void *liella_machine_rawmemchr_entry(const void *s, int c);
const void *liella_machine_memchr(const void *s, int c, size_t n);
/* The direct memchr entry (MEMCHR-FOLD): the gate on liella_machine_chr_limit, then the kernel above. */
const void *liella_machine_memchr_entry(const void *s, int c, size_t n);
size_t liella_machine_memcmp(const void *a, const void *b, size_t n);
int liella_machine_strcmp(const char *a, const char *b);
int liella_machine_strcmp_entry(const char *a, const char *b);
const char *liella_machine_strchr(const char *s, int c);
const char *liella_machine_strchrnul(const char *s, int c);
const void *liella_machine_memrchr(const void *s, int c, size_t n);
/* The direct memrchr entry (SCAN-RAW): the n and NULL tests, then the kernel above. */
const void *liella_machine_memrchr_entry(const void *s, int c, size_t n);
int liella_machine_strncmp(const char *a, const char *b, size_t n);
/* Selftest hooks (feature "selftest"; D-4.29): rescan counter, ymm15 upper-half probes. */
uint64_t liella_machine_strcmp_rescan_count(void);
void liella_machine_ymm15_dirty(void);
int liella_machine_ymm15_dirty_cmp(const char *a, const char *b);
uint64_t liella_machine_ymm15_upper(void);
size_t liella_machine_casecmp_avx2(const char *a, const char *b, size_t n);
size_t liella_machine_casecmp_sse2(const char *a, const char *b, size_t n);
int liella_machine_casecmp_ord_avx2(const char *a, const char *b, size_t n, const void *loc);
int liella_machine_casecmp_ord_sse2(const char *a, const char *b, size_t n, const void *loc);
size_t liella_machine_pairscan(const void *h, size_t count, size_t off, int b0, int b1);
size_t liella_machine_pairscan_ci(const void *h, size_t count, size_t off, int b0, int b1, unsigned fl);
size_t liella_machine_strpair(const char *p, size_t off, int c0, int c1);
size_t liella_machine_wcspair(const int *p, size_t off, int c0, int c1);
size_t liella_machine_env_find(const char *const *env, const char *name, size_t len);
/* env_find's scan answering also its stop slot (ENVIRON-SMALL-CYCLE): rax = the entry (0: none),
 * rdx = its slot's address, or the NULL slot's on a miss (0 when env is 0); boundary's ffi.rs
 * `EnvSlot` is the same #[repr(C)] pair. */
struct liella_env_slot { size_t entry, slot; };
struct liella_env_slot liella_machine_env_slot(const char *const *env, const char *name, size_t len);
/* The first index where two NUL-terminated strings differ or the first ends, and whether one of
 * the first string's bytes up to that index has bit 7 set (STRCOLL-2, STRCOLL-4; D-4.37).
 * Returned in rax (index) and rdx (high) by the SysV rule for a 16-byte struct of two INTEGER
 * eightbytes; boundary's ffi.rs `StrDiff` is the same #[repr(C)] pair. One member line keeps the
 * csys-audit's one-statement rule for brace bodies. */
struct liella_strdiff { size_t index, high; };
struct liella_strdiff liella_machine_strdiff(const char *a, const char *b);
void *liella_machine_copy_erms(void *dst, const void *src, size_t n);
void *liella_machine_copy_erms_a64(void *dst, const void *src, size_t n);
void liella_machine_copy_avx2(void *dst, const void *src, size_t n);
void liella_machine_fill_erms(void *dst, int c, size_t n);
/* Overlap-safe bulk moves and the AVX2 fill (MEM-BULK, MEM-FALLBACK; machine/scan.S): return d. */
void *liella_machine_move_avx2(void *d, const void *s, size_t n);
void *liella_machine_move_sse2(void *d, const void *s, size_t n);
void *liella_machine_fill_avx2(void *d, int c, size_t n);
void *liella_machine_fill_sse2(void *d, int c, size_t n);
/* The folded 1-64 byte copy and fill classes (D-4.22 MEM-K4b; src/machine/fold.S). */
void *liella_machine_move_fold33(void *d, const void *s, size_t n);
void *liella_machine_move_fold1(void *d, const void *s, size_t n);
void *liella_machine_fill_fold33(void *d, int c, size_t n);
void *liella_machine_fill_fold1(void *d, int c, size_t n);
/* The internal 65-256 byte move (COPY-256; src/machine/fold.S): no NULL tests, overlap allowed,
   d and s valid for n bytes, 65 <= n <= 256; returns nothing. */
void liella_machine_move_mid(void *d, const void *s, size_t n);
/* The direct entries exported as memmove/memcpy and memset (D-4.22 COPY-FILL; src/machine/fold.S): return d. */
void *liella_machine_memmove(void *d, const void *s, size_t n);
void *liella_machine_memset(void *d, int c, size_t n);
/* The direct compare entries exported as memcmp and bcmp/__memcmpeq (MEMCMP-FOLD; src/machine/fold.S). */
int liella_machine_compare(const void *a, const void *b, size_t n);
int liella_machine_equal(const void *a, const void *b, size_t n);
/* The direct entry exported as mempcpy and __mempcpy (D-4.22, WL-HOT-SEQ2; src/machine/fold.S): returns d + n. */
void *liella_machine_mempcpy(void *d, const void *s, size_t n);
void liella_machine_trap(void);
/* Threads, cancellation, non-local jumps and contexts (D-22.9, D-22.11, D-21.1; src/machine/thread_start.S,
 * setjmp.S, context.S). */
long liella_machine_clone_thread(unsigned long flags, uintptr_t stack, uint32_t *ptid, uint32_t *ctid, uintptr_t tls);
long liella_machine_clone_raw(unsigned long flags, uintptr_t stack, uintptr_t ptid, uintptr_t ctid, uintptr_t tls);
long liella_machine_clone3_run(const void *args, long size, void (*entry)(uintptr_t), uintptr_t arg);
long liella_machine_syscall_cancel(const uint32_t *word, long nr, long a, long b, long c, long d, long e, long f);
long liella_machine_syscall_cancel4(const uint32_t *word, long nr, long a, long b, long c, long d);
void liella_machine_cancel_bounds(uintptr_t out[3]);
/* The direct cancellation points (D-22.11 CANCEL-FAST-2; src/machine/points.S), exported as read, write,
 * pread/pread64, pwrite/pwrite64, close/__close; and the asynchronous-I/O flag close reads. */
long liella_machine_read(int fd, void *buf, size_t n);
long liella_machine_write(int fd, const void *buf, size_t n);
long liella_machine_pread(int fd, void *buf, size_t n, long off);
long liella_machine_pwrite(int fd, const void *buf, size_t n, long off);
int liella_machine_close(int fd);
extern unsigned char liella_machine_aio_used;
int liella_machine_sigsetjmp(void *env, int savemask);
void liella_machine_siglongjmp(void *env, int val);
void liella_machine_longjmp_nomask(void *env, int val);
int liella_machine_getcontext(void *uc);
int liella_machine_setcontext(const void *uc);
int liella_machine_swapcontext(void *save, const void *to);
void liella_machine_start_context(void);
/* Class adapter: va_arg accessor (src/adapter/variadic.c). */
long liella_va_long(void *ap);
double liella_va_double(void *ap);
void liella_va_ldouble(void *ap, long double *out);
/* Class adapter, runtime (src/adapter/runtime.c). */
uintptr_t liella_link_preinit_start(void);
uintptr_t liella_link_preinit_end(void);
uintptr_t liella_link_init_start(void);
uintptr_t liella_link_init_end(void);
uintptr_t liella_link_fini_start(void);
uintptr_t liella_link_fini_end(void);
uintptr_t liella_link_init(void);
uintptr_t liella_link_fini(void);
uint16_t liella_link_fpu_control(void);
uintptr_t liella_link_rela_iplt_start(void);
uintptr_t liella_link_rela_iplt_end(void);
/* Caller-owned memory through the (replaceable) malloc family's symbols (D-X.14). */
void *liella_caller_malloc(size_t n);
void *liella_caller_calloc(size_t k, size_t n);
void *liella_caller_realloc(void *p, size_t n);
void liella_caller_free(void *p);
/* The image's ELF header and a static executable's .eh_frame bounds (D-2.7), 0 where absent. */
uintptr_t liella_link_ehdr(void);
uintptr_t liella_link_eh_frame_start(void);
uintptr_t liella_link_eh_frame_end(void);

/* Class sys (generated from csys/syscalls.tsv). */
#include "liella_sys.h"

#endif
