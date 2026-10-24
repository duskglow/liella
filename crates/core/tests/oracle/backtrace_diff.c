/* backtrace end to end (spec §2.7; D-2.7): built against the host C library and against
 * Liella's products, the outputs must match line for line (backtrace_products.rs). Black-box:
 * frames are reported by which of this program's own functions contains each return address
 * (the two builds lay code out differently), and the texts of backtrace_symbols with the
 * addresses and the program's name normalised. */
#define _GNU_SOURCE
#include <execinfo.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static char obuf[1 << 16];
static size_t used;
static void flush_out(void) { if (used) { (void)!write(1, obuf, used); used = 0; } }
static void ch(char c) { if (used == sizeof obuf) flush_out(); obuf[used++] = c; }
static void str(const char *s) { while (*s) ch(*s++); }
static void num(long v) {
    char b[24]; int n = 0; unsigned long u = v < 0 ? -(unsigned long)v : (unsigned long)v;
    if (v < 0) ch('-');
    do { b[n++] = (char)('0' + u % 10); u /= 10; } while (u);
    while (n) ch(b[--n]);
}

/* The functions whose frames are expected. Each is in its own `.text.sorted.*` section, which
 * the default linker script places in name order, so their order in memory is fixed and each
 * range ends where the next function starts (checked below). */
#define NI(n) __attribute__((noinline, noclone, section(".text.sorted.liella_probe_" #n)))
static void *frames[64];
static int got;
static int depth_n = 64;
static volatile int sink;

NI(01) static void leaf(void) { got = backtrace(frames, depth_n); sink++; }
NI(02) static void f3(void) { leaf(); sink++; }
NI(03) static void f2(void) { f3(); sink++; }
NI(04) static void f1(void) { f2(); sink++; }

NI(05) static void on_signal(int sig) { (void)sig; leaf(); sink++; }
NI(06) static void g3(void) { raise(SIGUSR1); sink++; }
NI(07) static void g2(void) { g3(); sink++; }
NI(08) static void g1(void) { g2(); sink++; }

/* The probe's own entry point for each case; frames are reported up to it. */
static int which;
NI(09) static void driver(void) { if (which == 0) f1(); else g1(); sink++; }
NI(10) static void after_all(void) { sink++; }

struct fn { const char *name; uintptr_t start; };
static struct fn fns[16];
static int nfns;
static void add(const char *name, void *p) { fns[nfns].name = name; fns[nfns].start = (uintptr_t)p; nfns++; }

/* The name of the known function containing `a`: the greatest start at or below it, provided
 * that function is not the last one (whose end is unknown) and `a` is below the next start. */
static const char *where(void *a) {
    uintptr_t x = (uintptr_t)a;
    const char *best = NULL;
    uintptr_t bs = 0, next = UINTPTR_MAX;
    for (int i = 0; i < nfns; i++) {
        if (fns[i].start <= x && fns[i].start >= bs) { bs = fns[i].start; best = fns[i].name; }
    }
    for (int i = 0; i < nfns; i++) if (fns[i].start > bs && fns[i].start < next) next = fns[i].start;
    if (!best || next == UINTPTR_MAX || x >= next) return "other";
    return best;
}

static void show(const char *label) {
    /* The total counts the library's own start-up frames, which differ by library: it is shown
     * only where the limit cuts the trace inside this program. */
    str(label);
    if (strncmp(label, "limit", 5) == 0) { str(": n="); num(got); } else str(":");
    /* Frames up to main; then whether more follow (the start code differs by library). */
    int i = 0;
    /* Frames up to driver; a run of frames outside this program's functions (the library's
     * own, the signal trampoline) shows as one `~`. */
    int other = 0;
    for (; i < got; i++) {
        const char *w = where(frames[i]);
        if (strcmp(w, "other") == 0) { if (!other) { str(" ~"); other = 1; } continue; }
        other = 0;
        ch(' '); str(w);
        if (strcmp(w, "driver") == 0) { i++; break; }
    }
    str(" | after_driver="); num(got - i > 0);
    ch('\n');
}

/* A backtrace_symbols text with each hexadecimal number shown as H and the program name as P. */
static void normalised(const char *s, const char *prog) {
    size_t pl = strlen(prog);
    while (*s) {
        if (strncmp(s, prog, pl) == 0) { ch('P'); s += pl; continue; }
        if (s[0] == '0' && s[1] == 'x') {
            s += 2;
            while ((*s >= '0' && *s <= '9') || (*s >= 'a' && *s <= 'f')) s++;
            ch('H');
            continue;
        }
        ch(*s++);
    }
}

int main(int argc, char **argv) {
    (void)argc;
    add("leaf", (void *)leaf); add("f3", (void *)f3); add("f2", (void *)f2); add("f1", (void *)f1);
    add("on_signal", (void *)on_signal); add("g3", (void *)g3); add("g2", (void *)g2); add("g1", (void *)g1);
    add("driver", (void *)driver); add("after_all", (void *)after_all);
    /* driver must not be the highest function, so its range is bounded by after_all. */
    str("layout_ok="); num((uintptr_t)driver < (uintptr_t)after_all); ch('\n');

    driver();
    show("nested");
    int sizes[] = { 0, 1, 2, 3, 5 };
    for (int k = 0; k < 5; k++) {
        depth_n = sizes[k];
        memset(frames, 0, sizeof frames);
        driver();
        char label[32];
        snprintf(label, sizeof label, "limit %d", sizes[k]);
        show(label);
    }
    depth_n = 64;

    signal(SIGUSR1, on_signal);
    which = 1;
    driver();
    which = 0;
    show("signal handler");

    /* backtrace_symbols: one block; each text normalised. */
    depth_n = 4;
    driver();
    const char *prog = strrchr(argv[0], '/') ? strrchr(argv[0], '/') + 1 : argv[0];
    char **syms = backtrace_symbols(frames, got);
    str("symbols: "); num(syms != NULL); ch('\n');
    for (int i = 0; syms && i < got; i++) { str("  "); normalised(syms[i], prog); ch('\n'); }
    /* The strings lie inside the same block as the pointer array. */
    if (syms) {
        uintptr_t lo = (uintptr_t)syms, hi = lo + 4096;
        int inside = 1;
        for (int i = 0; i < got; i++) if ((uintptr_t)syms[i] < lo || (uintptr_t)syms[i] > hi) inside = 0;
        str("one block: "); num(inside); ch('\n');
        free(syms);
    }
    str("symbols n=0: "); num(backtrace_symbols(frames, 0) != NULL); ch('\n');

    /* backtrace_symbols_fd to a pipe. */
    int p[2];
    (void)!pipe(p);
    backtrace_symbols_fd(frames, got, p[1]);
    close(p[1]);
    char b[4096];
    ssize_t n = read(p[0], b, sizeof b - 1);
    close(p[0]);
    b[n > 0 ? n : 0] = 0;
    str("symbols_fd:\n");
    char *line = b;
    while (*line) {
        char *e = strchr(line, '\n');
        if (e) *e = 0;
        str("  "); normalised(line, prog); ch('\n');
        if (!e) break;
        line = e + 1;
    }
    after_all();
    flush_out();
    return 0;
}
