/* Class adapter, floating ABI (OD-C4): the exports whose parameters or results are `long double`
 * (x87, `_Float64x`) or `_Float128`. Rust cannot name those types, so each export is one
 * statement that converts between the C type and a plain bit struct (through a union compound
 * literal) and calls the Rust implementation (`liella_*_bits`, crates/boundary/src/posix/
 * s08_numeric.rs), which works on the bits. All logic stays in Rust.
 *
 * Names are the exported glibc names under LIELLA_EXPORT, and `liella_fwd_<name>` otherwise
 * (test builds never interpose on the host libc, but the forwarders still compile). Locale and
 * grouping arguments are ignored: every locale is the C locale until phase 7 (D-6.7). */
#include <stddef.h>
#include <stdint.h>
#include "liella_csys.h"

/* The bit structs the Rust side takes and returns, and its entry points. */
#ifdef LIELLA_EXPORT
#define LIELLA_FWD(name) name
#else
#define LIELLA_FWD(name) liella_fwd_##name
#endif
struct liella_ld_bits { uint64_t mant; uint16_t se; uint16_t pad[3]; };
struct liella_f128_bits { uint64_t lo; uint64_t hi; };
union liella_ld { long double value; struct liella_ld_bits bits; };
union liella_f128 { __float128 value; struct liella_f128_bits bits; };
_Static_assert(sizeof(long double) == sizeof(struct liella_ld_bits), "long double is 16 bytes");
_Static_assert(sizeof(__float128) == sizeof(struct liella_f128_bits), "_Float128 is 16 bytes");
struct liella_ld_bits liella_strtold_bits(const char *s, char **e);
struct liella_ld_bits liella_wcstold_bits(const wchar_t *s, wchar_t **e);
struct liella_f128_bits liella_strtof128_bits(const char *s, char **e);
struct liella_f128_bits liella_wcstof128_bits(const wchar_t *s, wchar_t **e);
int liella_strfroml_bits(char *buf, size_t n, const char *fmt, struct liella_ld_bits x);
int liella_strfromf128_bits(char *buf, size_t n, const char *fmt, struct liella_f128_bits x);
char *liella_qecvt_bits(struct liella_ld_bits x, int ndigit, int *decpt, int *sign);
char *liella_qfcvt_bits(struct liella_ld_bits x, int ndigit, int *decpt, int *sign);
int liella_qecvt_r_bits(struct liella_ld_bits x, int ndigit, int *decpt, int *sign, char *buf, size_t len);
int liella_qfcvt_r_bits(struct liella_ld_bits x, int ndigit, int *decpt, int *sign, char *buf, size_t len);
char *liella_qgcvt_bits(struct liella_ld_bits x, int ndigit, char *buf);


#define LD(expr) (((union liella_ld){ .bits = (expr) }).value)
#define LD_BITS(x) (((union liella_ld){ .value = (x) }).bits)
#define F128(expr) (((union liella_f128){ .bits = (expr) }).value)
#define F128_BITS(x) (((union liella_f128){ .value = (x) }).bits)
#define UNUSED __attribute__((unused))

/* Text to long double (§8.2, §8.3). */
long double LIELLA_FWD(strtold)(const char *s, char **e) { return LD(liella_strtold_bits(s, e)); }
long double LIELLA_FWD(strtold_l)(const char *s, char **e, UNUSED void *loc) { return LD(liella_strtold_bits(s, e)); }
long double LIELLA_FWD(__strtold_l)(const char *s, char **e, UNUSED void *loc) { return LD(liella_strtold_bits(s, e)); }
long double LIELLA_FWD(__strtold_internal)(const char *s, char **e, UNUSED int group) { return LD(liella_strtold_bits(s, e)); }
long double LIELLA_FWD(strtof64x)(const char *s, char **e) { return LD(liella_strtold_bits(s, e)); }
long double LIELLA_FWD(strtof64x_l)(const char *s, char **e, UNUSED void *loc) { return LD(liella_strtold_bits(s, e)); }
long double LIELLA_FWD(wcstold)(const wchar_t *s, wchar_t **e) { return LD(liella_wcstold_bits(s, e)); }
long double LIELLA_FWD(wcstold_l)(const wchar_t *s, wchar_t **e, UNUSED void *loc) { return LD(liella_wcstold_bits(s, e)); }
long double LIELLA_FWD(__wcstold_l)(const wchar_t *s, wchar_t **e, UNUSED void *loc) { return LD(liella_wcstold_bits(s, e)); }
long double LIELLA_FWD(__wcstold_internal)(const wchar_t *s, wchar_t **e, UNUSED int group) { return LD(liella_wcstold_bits(s, e)); }
long double LIELLA_FWD(wcstof64x)(const wchar_t *s, wchar_t **e) { return LD(liella_wcstold_bits(s, e)); }
long double LIELLA_FWD(wcstof64x_l)(const wchar_t *s, wchar_t **e, UNUSED void *loc) { return LD(liella_wcstold_bits(s, e)); }

/* Text to _Float128. */
__float128 LIELLA_FWD(strtof128)(const char *s, char **e) { return F128(liella_strtof128_bits(s, e)); }
__float128 LIELLA_FWD(strtof128_l)(const char *s, char **e, UNUSED void *loc) { return F128(liella_strtof128_bits(s, e)); }
__float128 LIELLA_FWD(__strtof128_internal)(const char *s, char **e, UNUSED int group) { return F128(liella_strtof128_bits(s, e)); }
__float128 LIELLA_FWD(wcstof128)(const wchar_t *s, wchar_t **e) { return F128(liella_wcstof128_bits(s, e)); }
__float128 LIELLA_FWD(wcstof128_l)(const wchar_t *s, wchar_t **e, UNUSED void *loc) { return F128(liella_wcstof128_bits(s, e)); }
__float128 LIELLA_FWD(__wcstof128_internal)(const wchar_t *s, wchar_t **e, UNUSED int group) { return F128(liella_wcstof128_bits(s, e)); }

/* Floating to text (§8.4). */
int LIELLA_FWD(strfroml)(char *buf, size_t n, const char *fmt, long double x) { return liella_strfroml_bits(buf, n, fmt, LD_BITS(x)); }
int LIELLA_FWD(strfromf64x)(char *buf, size_t n, const char *fmt, long double x) { return liella_strfroml_bits(buf, n, fmt, LD_BITS(x)); }
int LIELLA_FWD(strfromf128)(char *buf, size_t n, const char *fmt, __float128 x) { return liella_strfromf128_bits(buf, n, fmt, F128_BITS(x)); }
char *LIELLA_FWD(qecvt)(long double x, int ndigit, int *decpt, int *sign) { return liella_qecvt_bits(LD_BITS(x), ndigit, decpt, sign); }
char *LIELLA_FWD(qfcvt)(long double x, int ndigit, int *decpt, int *sign) { return liella_qfcvt_bits(LD_BITS(x), ndigit, decpt, sign); }
int LIELLA_FWD(qecvt_r)(long double x, int ndigit, int *decpt, int *sign, char *buf, size_t len) { return liella_qecvt_r_bits(LD_BITS(x), ndigit, decpt, sign, buf, len); }
int LIELLA_FWD(qfcvt_r)(long double x, int ndigit, int *decpt, int *sign, char *buf, size_t len) { return liella_qfcvt_r_bits(LD_BITS(x), ndigit, decpt, sign, buf, len); }
char *LIELLA_FWD(qgcvt)(long double x, int ndigit, char *buf) { return liella_qgcvt_bits(LD_BITS(x), ndigit, buf); }
