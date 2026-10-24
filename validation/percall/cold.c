/* One-off ("cold") first-call costs that every short-lived process pays once (COLD-FIRST-CALLS, plan X-4).
 *   bash validation/wl_percall.sh --base none --spec validation/percall/cold.c --locale C <dir|rev>
 * cold_* cases make their call ONCE, only when n > 0, so I(N) - I(0) is the cost of that one first call
 * (every run is a fresh process). wl_percall divides by n * per = 20000 (per = 1): multiply a cold_* row
 * by 20000 to get instructions per process (resolution 0.01 x 20000 = 200 instructions).
 * warm_* cases are ordinary per-call loops (the cbench "second_*" rows), not multiplied.
 * Every case fixes its own locale with CASE_LOCALE, so --locale only selects LC_ALL for cold_setlocale_env
 * (setlocale(LC_ALL, "") after the harness has set "C": the C -> C repeat when --locale C).
 * Time zone: TZ is not set by the harness; the zone is the host's /etc/localtime (keke: Etc/UTC), the
 * same file for glibc and Liella, like the workloads. */
#include "percall.h"
#include <libintl.h>
#include <time.h>

static void pc_textdomain_once(void) {
    bindtextdomain("coreutils", "/usr/share/locale");
    textdomain("coreutils");
    pc_sink += (long)gettext("memory exhausted")[0];
}

CASE_LOCALE(cold_setlocale_enUS, 1, 1, "C") {
    if (n) pc_sink += (long)setlocale(LC_ALL, "en_US.UTF-8");
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, "en_US.UTF-8") ? "set" : "fail");
}
CASE_LOCALE(cold_setlocale_CUTF8, 1, 1, "C") {
    if (n) pc_sink += (long)setlocale(LC_ALL, "C.UTF-8");
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, "C.UTF-8") ? "set" : "fail");
}
CASE_LOCALE(cold_setlocale_env, 1, 1, "C") {
    if (n) pc_sink += (long)setlocale(LC_ALL, "");
    PC_CHECKING pc_check("%s", setlocale(LC_ALL, "") ? "set" : "fail");
}
CASE_LOCALE(warm_setlocale_enUS, 1, 1, "en_US.UTF-8") {
    for (long i = 0; i < n; i++) pc_sink += (long)setlocale(LC_ALL, "en_US.UTF-8");
}
CASE_LOCALE(cold_textdomain_C, 1, 1, "C") {
    if (n) pc_textdomain_once();
    PC_CHECKING pc_check("%s", gettext("memory exhausted"));
}
CASE_LOCALE(cold_textdomain_CUTF8, 1, 1, "C.UTF-8") {
    if (n) pc_textdomain_once();
    PC_CHECKING pc_check("%s", gettext("memory exhausted"));
}
CASE_LOCALE(cold_textdomain_enUS, 1, 1, "en_US.UTF-8") {
    if (n) pc_textdomain_once();
    PC_CHECKING pc_check("%s", gettext("memory exhausted"));
}
CASE_LOCALE(warm_bindtextdomain_gettext_enUS, 1, 1, "en_US.UTF-8") {
    pc_textdomain_once();
    for (long i = 0; i < n; i++) {
        bindtextdomain("coreutils", "/usr/share/locale");
        pc_sink += (long)gettext("memory exhausted")[0];
    }
}
/* The two halves of the case above (GETTEXT-NOCATALOG-PERCALL): the unchanged binding alone, and
 * the lookup with no catalog alone. */
CASE_LOCALE(warm_bindtextdomain_same_enUS, 1, 1, "en_US.UTF-8") {
    pc_textdomain_once();
    for (long i = 0; i < n; i++)
        pc_sink += (long)bindtextdomain("coreutils", "/usr/share/locale")[0];
}
CASE_LOCALE(warm_gettext_nocatalog_enUS, 1, 1, "en_US.UTF-8") {
    pc_textdomain_once();
    for (long i = 0; i < n; i++)
        pc_sink += (long)gettext("memory exhausted")[0];
}
CASE_LOCALE(cold_localtime_r, 1, 1, "C") {
    time_t t = 1700000000;
    struct tm tm;
    if (n) { localtime_r(&t, &tm); pc_sink += tm.tm_hour; }
    PC_CHECKING { localtime_r(&t, &tm); pc_check("%d %d", tm.tm_hour, tm.tm_isdst); }
}
CASE_LOCALE(cold_mktime, 1, 1, "C") { /* the first mktime after the zone is loaded (cbench first_mktime) */
    time_t t = 1700000000;
    struct tm tm;
    localtime_r(&t, &tm);
    if (n) pc_sink += (long)mktime(&tm);
    PC_CHECKING pc_check("%ld", (long)mktime(&tm));
}
CASE_LOCALE(warm_mktime, 1, 1, "C") {
    time_t t = 1700000000;
    struct tm tm;
    localtime_r(&t, &tm);
    for (long i = 0; i < n; i++) pc_sink += (long)mktime(&tm);
}
