/* wl_percall gettext set (PERF-GETTEXT-LANGUAGE-STEADY): the gettext rows of bench/c/cbench_opt.c, one call each,
 * with the same fixture (a three-entry catalog "cbench" under <tmp>/en_US/LC_MESSAGES, bound and made the default
 * domain) in en_US.UTF-8 (`wl_percall.sh --spec validation/percall/gettext.c`):
 *   gettext_found             LANGUAGE unset, gettext("hello") -> "hallo"
 *   ngettext_found            LANGUAGE unset, ngettext("apple", "apples", 3) -> the second plural form
 *   gettext_language_steady   LANGUAGE=de:en_US set once (putenv of a fixed string), gettext("hello")
 *   gettext_language_alternating  putenv of LANGUAGE=en_US / LANGUAGE=de:en_US in turn, then gettext("hello")
 * Typical-call counts: a translated program's messages (a few hundred per run). Every case builds its own fixture in
 * its setup (both the n = 0 and the n = N run, so it cancels) and removes it at the end. */
#include "percall.h"
#include <libintl.h>
#include <stdint.h>

struct gt_fixture { char top[64], l1[96], l2[112], mo[144]; int ok; };

static void gt_put32(unsigned char *p, uint32_t v)
{
    p[0] = (unsigned char)v; p[1] = (unsigned char)(v >> 8); p[2] = (unsigned char)(v >> 16); p[3] = (unsigned char)(v >> 24);
}

/* The catalog of cbench_opt.c's write_mo: header, "apple"/"apples" -> "Apfel"/"Äpfel", "hello" -> "hallo". */
static int gt_write_mo(const char *path)
{
    static const char header[] = "Content-Type: text/plain; charset=UTF-8\nPlural-Forms: nplurals=2; plural=n != 1;\n";
    struct { const char *s; uint32_t len; } orig[3] = {{"", 0}, {"apple\0apples", 12}, {"hello", 5}},
        trans[3] = {{header, sizeof header - 1}, {"Apfel\0\xc3\x84pfel", 12}, {"hallo", 5}};
    unsigned char buf[512];
    memset(buf, 0, sizeof buf);
    uint32_t n = 3, o = 28, t = o + 8 * n, at = t + 8 * n;
    gt_put32(buf, 0x950412de); gt_put32(buf + 8, n); gt_put32(buf + 12, o); gt_put32(buf + 16, t); gt_put32(buf + 24, at);
    for (uint32_t i = 0; i < n; i++) {
        gt_put32(buf + o + 8 * i, orig[i].len); gt_put32(buf + o + 8 * i + 4, at);
        memcpy(buf + at, orig[i].s, orig[i].len); at += orig[i].len + 1;
    }
    for (uint32_t i = 0; i < n; i++) {
        gt_put32(buf + t + 8 * i, trans[i].len); gt_put32(buf + t + 8 * i + 4, at);
        memcpy(buf + at, trans[i].s, trans[i].len); at += trans[i].len + 1;
    }
    int f = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (f < 0) return 0;
    int ok = write(f, buf, at) == (ssize_t)at;
    close(f);
    return ok;
}

static void gt_setup(struct gt_fixture *x)
{
    unsetenv("LANGUAGE");
    snprintf(x->top, sizeof x->top, "/tmp/pc_gettext.XXXXXX");
    x->ok = mkdtemp(x->top) != NULL;
    snprintf(x->l1, sizeof x->l1, "%s/en_US", x->top);
    snprintf(x->l2, sizeof x->l2, "%s/en_US/LC_MESSAGES", x->top);
    snprintf(x->mo, sizeof x->mo, "%s/en_US/LC_MESSAGES/cbench.mo", x->top);
    x->ok = x->ok && mkdir(x->l1, 0700) == 0 && mkdir(x->l2, 0700) == 0 && gt_write_mo(x->mo) &&
            bindtextdomain("cbench", x->top) && textdomain("cbench");
}

static void gt_done(struct gt_fixture *x)
{
    unsetenv("LANGUAGE");
    unlink(x->mo); rmdir(x->l2); rmdir(x->l1); rmdir(x->top);
}

static char *(*volatile gt_gettext)(const char *) = gettext;
static char *(*volatile gt_ngettext)(const char *, const char *, unsigned long) = ngettext;
static char gt_lang_a[] = "LANGUAGE=en_US", gt_lang_b[] = "LANGUAGE=de:en_US";

CASE(gettext_found, 300, 1) {
    struct gt_fixture x; gt_setup(&x);
    pc_sink += (long)(uintptr_t)gt_gettext("hello");
    for (long i = 0; i < n; i++) pc_sink += (long)(uintptr_t)gt_gettext("hello");
    PC_CHECKING pc_check("%d %s", x.ok, gt_gettext("hello"));
    gt_done(&x);
}
CASE(ngettext_found, 100, 1) {
    struct gt_fixture x; gt_setup(&x);
    pc_sink += (long)(uintptr_t)gt_ngettext("apple", "apples", 3);
    for (long i = 0; i < n; i++) pc_sink += (long)(uintptr_t)gt_ngettext("apple", "apples", 3);
    PC_CHECKING pc_check("%d %s %s", x.ok, gt_ngettext("apple", "apples", 3), gt_ngettext("apple", "apples", 1));
    gt_done(&x);
}
CASE(gettext_language_steady, 300, 1) {
    struct gt_fixture x; gt_setup(&x);
    putenv(gt_lang_b);
    pc_sink += (long)(uintptr_t)gt_gettext("hello");
    for (long i = 0; i < n; i++) pc_sink += (long)(uintptr_t)gt_gettext("hello");
    PC_CHECKING pc_check("%d %s", x.ok, gt_gettext("hello"));
    gt_done(&x);
}
CASE(gettext_language_alternating, 10, 1) {
    struct gt_fixture x; gt_setup(&x);
    putenv(gt_lang_a); pc_sink += (long)(uintptr_t)gt_gettext("hello");
    putenv(gt_lang_b); pc_sink += (long)(uintptr_t)gt_gettext("hello");
    for (long i = 0; i < n; i++) pc_sink += (long)(uintptr_t)(putenv((i & 1) ? gt_lang_a : gt_lang_b), gt_gettext("hello"));
    PC_CHECKING { putenv(gt_lang_a); const char *a = gt_gettext("hello"); putenv(gt_lang_b); pc_check("%d %s %s", x.ok, a, gt_gettext("hello")); }
    gt_done(&x);
}
