/* wl_percall probe spec API. A spec is a C file: #include "percall.h", then one CASE per measured call.
 *   CASE(name, calls, per) { setup; for (long i = 0; i < n; i++) { the call to count } }
 * name: identifier; calls: typical number of such calls in a workload (0 = unknown; sorts the table);
 * per: calls made by ONE loop iteration (1 usually; the entries of a directory for a readdir pass), the
 * result is divided by it. The harness runs the case with n = 0 (setup only) and n = N and reports
 * (instructions:u(N) - instructions:u(0)) / (N * per). dir: a directory of 38 files plus "a" (stat target),
 * "text" (200 lines) is passed in; write results to pc_sink so the compiler keeps them.
 * LOCALE (PERCALL-LOCALE): before a case runs, the harness calls setlocale(LC_ALL, "") - the locale of the
 * environment, which wl_percall sets to LC_ALL=en_US.UTF-8 (--locale L for another) - like every real program
 * that starts with setlocale(LC_ALL, ""). A case that must run in one fixed locale says so:
 *   CASE_LOCALE(name, calls, per, "C") { ... }   (setlocale(LC_ALL, "C") instead).
 * If setlocale fails (locale not installed) the harness exits 4 instead of measuring in "C". A case may still
 * call setlocale itself. `percall --locale` prints "<setlocale(LC_ALL, "")> <nl_langinfo(CODESET)>"; wl_percall
 * prints it in the header and refuses to measure when it disagrees with the requested locale.
 * SELF-CHECK (TOOLS-PERCALL): after the loop, `PC_CHECKING { r = the call once more; pc_check("fmt", r...); }`
 * prints a line in check mode only (`percall <case> check <dir>`; outside it costs the same in n = 0 and n = N, so
 * it cancels). wl_percall compares the lines of glibc and Liella: a reference that computes a different answer
 * is not a reference (row marked MISMATCH). Print only what both must agree on (signs, counts, strings; never
 * pointers). Arguments of the checked call go through volatile pointers so gcc cannot fold the call. */
#ifndef PERCALL_H
#define PERCALL_H
#define _GNU_SOURCE
#include <dirent.h>
#include <fcntl.h>
#include <langinfo.h>
#include <locale.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
/* The section holds one POINTER per case: 8-byte entries pack without padding (gcc gives structs of >= 32 bytes
 * 32-byte alignment, which would leave gaps between section entries). */
struct pc_case { const char *name; long calls; long per; void (*fn)(long, const char *); const char *locale; };
extern volatile long pc_sink;
extern volatile int pc_check_mode;
void pc_check(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
#define PC_CHECKING if (pc_check_mode)
#define PC_SIGN(x) ((x) > 0 ? 1 : (x) < 0 ? -1 : 0)
#define CASE_LOCALE(nm, ncalls, nper, loc) \
    static void pc_##nm(long n, const char *dir); \
    static struct pc_case pc_r_##nm = {#nm, ncalls, nper, pc_##nm, loc}; \
    static struct pc_case *const pc_p_##nm __attribute__((used, section("pc_cases"))) = &pc_r_##nm; \
    static void pc_##nm(long n, const char *dir)
#define CASE(nm, ncalls, nper) CASE_LOCALE(nm, ncalls, nper, "")
#endif
