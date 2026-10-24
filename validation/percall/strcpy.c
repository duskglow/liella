/* STRCPY-SHORT / STRCPY-FAMILY string copies: strcpy/stpcpy/strncpy and the _chk copies on short
 * names (bash-loop: 21M strcpy calls). The _chk copies are the fortified entries (exact room). */
#include "percall.h"

char *__strcpy_chk(char *d, const char *s, size_t room);
char *__stpcpy_chk(char *d, const char *s, size_t room);

static char src_buf[128] = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyz";
static char *volatile src_v = src_buf;
static char dst_buf[160];
static char *volatile dst_v = dst_buf;

static const char *with_len(int len) { char *s = src_v; s[len] = 0; return s; }

#define COPY_BODY(call, len) \
    const char *s = with_len(len); \
    for (long i = 0; i < n; i++) { char *const volatile *dp = &dst_v; pc_sink += (long)(call(*dp, s) != 0); } \
    PC_CHECKING pc_check("%d %d", (int)strlen(dst_v), dst_v[len]);
#define CHK_BODY(call, len) \
    const char *s = with_len(len); \
    for (long i = 0; i < n; i++) { char *const volatile *dp = &dst_v; pc_sink += (long)(call(*dp, s, (len) + 1) != 0); } \
    PC_CHECKING pc_check("%d %d", (int)strlen(dst_v), dst_v[len]);
#define NCOPY_BODY(len, nn) \
    const char *s = with_len(len); \
    for (long i = 0; i < n; i++) { char *const volatile *dp = &dst_v; pc_sink += (long)(strncpy(*dp, s, nn) != 0); } \
    PC_CHECKING pc_check("%d %d", (int)strlen(dst_v), dst_v[nn - 1]);

CASE(strcpy_8, 21000000, 1) { COPY_BODY(strcpy, 8) }
CASE(strcpy_16, 0, 1) { COPY_BODY(strcpy, 16) }
CASE(strcpy_40, 0, 1) { COPY_BODY(strcpy, 40) }
CASE(strcpy_80, 0, 1) { COPY_BODY(strcpy, 80) }
CASE(stpcpy_8, 0, 1) { COPY_BODY(stpcpy, 8) }
CASE(stpcpy_12, 0, 1) { COPY_BODY(stpcpy, 12) }
CASE(stpcpy_16, 0, 1) { COPY_BODY(stpcpy, 16) }
CASE(stpcpy_40, 0, 1) { COPY_BODY(stpcpy, 40) }
CASE(stpcpy_80, 0, 1) { COPY_BODY(stpcpy, 80) }
CASE(strcpy_chk_8, 0, 1) { CHK_BODY(__strcpy_chk, 8) }
CASE(strcpy_chk_16, 0, 1) { CHK_BODY(__strcpy_chk, 16) }
CASE(strcpy_chk_40, 0, 1) { CHK_BODY(__strcpy_chk, 40) }
CASE(strcpy_chk_80, 0, 1) { CHK_BODY(__strcpy_chk, 80) }
CASE(stpcpy_chk_8, 0, 1) { CHK_BODY(__stpcpy_chk, 8) }
CASE(stpcpy_chk_16, 0, 1) { CHK_BODY(__stpcpy_chk, 16) }
CASE(stpcpy_chk_40, 0, 1) { CHK_BODY(__stpcpy_chk, 40) }
CASE(stpcpy_chk_80, 0, 1) { CHK_BODY(__stpcpy_chk, 80) }
CASE(strncpy_12_32, 1000000, 1) { NCOPY_BODY(12, 32) }
CASE(strncpy_40_32, 0, 1) { NCOPY_BODY(40, 32) }
/* STRNCPY-SHORT: the WL-STAKES-2 shapes (bash-loop: n == len 25 and 7) through a volatile
 * function pointer, plus n < len, n > len (padding) and n == 0. */
static __typeof__(strncpy) *volatile P_strncpy = strncpy;
#define PNCOPY_BODY(len, nn) \
    const char *s = with_len(len); static volatile size_t vn; vn = (nn); \
    for (long i = 0; i < n; i++) { char *const volatile *dp = &dst_v; pc_sink += (long)(P_strncpy(*dp, s, vn) != 0); } \
    PC_CHECKING pc_check("%d %d", (int)strnlen(dst_v, (nn) + 1), (nn) ? dst_v[(nn) ? (nn) - 1 : 0] : 0);
CASE(strncpy_25_25, 300000, 1) { PNCOPY_BODY(25, 25) }
CASE(strncpy_7_7, 300000, 1) { PNCOPY_BODY(7, 7) }
CASE(strncpy_40_12, 0, 1) { PNCOPY_BODY(40, 12) }
CASE(strncpy_5_48, 0, 1) { PNCOPY_BODY(5, 48) }
CASE(strncpy_9_0, 0, 1) { PNCOPY_BODY(9, 0) }
/* STRNCPY-FAMILY: stpncpy (returns the end) and the fortified __strncpy_chk (destlen checked
 * first) on the same shapes, through volatile function pointers. */
extern char *__strncpy_chk(char *, const char *, size_t, size_t);
static __typeof__(stpncpy) *volatile P_stpncpy = stpncpy;
static __typeof__(__strncpy_chk) *volatile P_strncpy_chk = __strncpy_chk;
#define PSTPN_BODY(len, nn) \
    const char *s = with_len(len); static volatile size_t vn; vn = (nn); long end = 0; \
    for (long i = 0; i < n; i++) { char *const volatile *dp = &dst_v; char *r = P_stpncpy(*dp, s, vn); end = r - *dp; pc_sink += end; } \
    PC_CHECKING pc_check("%ld %d %d", end, (int)strnlen(dst_v, (nn) + 1), (nn) ? dst_v[(nn) - 1] : 0);
#define PCHK_BODY(len, nn) \
    const char *s = with_len(len); static volatile size_t vn; vn = (nn); \
    for (long i = 0; i < n; i++) { char *const volatile *dp = &dst_v; pc_sink += (long)(P_strncpy_chk(*dp, s, vn, sizeof dst_buf) != 0); } \
    PC_CHECKING pc_check("%d %d", (int)strnlen(dst_v, (nn) + 1), (nn) ? dst_v[(nn) - 1] : 0);
CASE(stpncpy_25_25, 0, 1) { PSTPN_BODY(25, 25) }
CASE(stpncpy_7_7, 0, 1) { PSTPN_BODY(7, 7) }
CASE(stpncpy_40_12, 0, 1) { PSTPN_BODY(40, 12) }
CASE(stpncpy_5_48, 0, 1) { PSTPN_BODY(5, 48) }
CASE(strncpy_chk_25_25, 0, 1) { PCHK_BODY(25, 25) }
CASE(strncpy_chk_7_7, 0, 1) { PCHK_BODY(7, 7) }
CASE(strncpy_chk_40_12, 0, 1) { PCHK_BODY(40, 12) }
CASE(strncpy_chk_5_48, 0, 1) { PCHK_BODY(5, 48) }
/* STPNCPY-ALIAS: __stpncpy (the stpncpy entry) and the fortified __stpncpy_chk (destlen checked
 * first) on the same shapes, through volatile function pointers. */
extern char *__stpncpy(char *, const char *, size_t);
extern char *__stpncpy_chk(char *, const char *, size_t, size_t);
static __typeof__(__stpncpy) *volatile P___stpncpy = __stpncpy;
static __typeof__(__stpncpy_chk) *volatile P_stpncpy_chk = __stpncpy_chk;
#define PSTPNA_BODY(len, nn)     const char *s = with_len(len); static volatile size_t vn; vn = (nn); long end = 0;     for (long i = 0; i < n; i++) { char *const volatile *dp = &dst_v; char *r = P___stpncpy(*dp, s, vn); end = r - *dp; pc_sink += end; }     PC_CHECKING pc_check("%ld %d %d", end, (int)strnlen(dst_v, (nn) + 1), (nn) ? dst_v[(nn) - 1] : 0);
#define PSTPNCHK_BODY(len, nn)     const char *s = with_len(len); static volatile size_t vn; vn = (nn); long end = 0;     for (long i = 0; i < n; i++) { char *const volatile *dp = &dst_v; char *r = P_stpncpy_chk(*dp, s, vn, sizeof dst_buf); end = r - *dp; pc_sink += end; }     PC_CHECKING pc_check("%ld %d %d", end, (int)strnlen(dst_v, (nn) + 1), (nn) ? dst_v[(nn) - 1] : 0);
CASE(stpncpy_alias_25_25, 0, 1) { PSTPNA_BODY(25, 25) }
CASE(stpncpy_alias_7_7, 0, 1) { PSTPNA_BODY(7, 7) }
CASE(stpncpy_alias_40_12, 0, 1) { PSTPNA_BODY(40, 12) }
CASE(stpncpy_alias_5_48, 0, 1) { PSTPNA_BODY(5, 48) }
CASE(stpncpy_chk_25_25, 0, 1) { PSTPNCHK_BODY(25, 25) }
CASE(stpncpy_chk_7_7, 0, 1) { PSTPNCHK_BODY(7, 7) }
CASE(stpncpy_chk_40_12, 0, 1) { PSTPNCHK_BODY(40, 12) }
CASE(stpncpy_chk_5_48, 0, 1) { PSTPNCHK_BODY(5, 48) }
