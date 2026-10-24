/* FILE-LOCK-2 stream byte ops, single-threaded and with a second thread alive (_mt: the FILE lock is taken) */
#include "percall.h"
#include <pthread.h>

static int mt_pipe[2];
static void *mt_idle(void *a) { char c; (void)a; while (read(mt_pipe[0], &c, 1) < 0) {} return 0; }
static void go_mt(void) {
    static int done;
    if (done) return;
    done = 1;
    pthread_t t;
    if (pipe(mt_pipe) != 0 || pthread_create(&t, 0, mt_idle, 0) != 0) abort();
}

#define GETC_BODY \
    char p[512]; snprintf(p, sizeof p, "%s/text", dir); \
    FILE *f = fopen(p, "r"); \
    for (long i = 0; i < n; i++) { int c = getc(f); if (c == EOF) { rewind(f); c = getc(f); } pc_sink += c; } \
    PC_CHECKING { rewind(f); long k = 0, s = 0; int c; while ((c = getc(f)) != EOF) { k++; s += c; } pc_check("%ld %ld", k, s); }
#define PUTC_BODY \
    FILE *f = fopen("/dev/null", "w"); \
    for (long i = 0; i < n; i++) pc_sink += putc('x', f); \
    PC_CHECKING pc_check("%d", putc('x', f));
#define FPUTS_BODY \
    FILE *f = fopen("/dev/null", "w"); \
    for (long i = 0; i < n; i++) pc_sink += fputs("hello world\n", f); \
    PC_CHECKING pc_check("%d", PC_SIGN(fputs("hello world\n", f)));
#define FGETS_BODY \
    char p[512], b[256]; snprintf(p, sizeof p, "%s/text", dir); \
    FILE *f = fopen(p, "r"); \
    for (long i = 0; i < n; i++) { rewind(f); while (fgets(b, sizeof b, f)) pc_sink += b[0]; } \
    PC_CHECKING { long k = 0, l = 0; rewind(f); while (fgets(b, sizeof b, f)) { k++; l += (long)strlen(b); } pc_check("%ld %ld", k, l); }
#define FLOCK_BODY \
    FILE *f = fopen("/dev/null", "w"); \
    for (long i = 0; i < n; i++) { flockfile(f); flockfile(f); pc_sink += putc_unlocked('x', f); funlockfile(f); funlockfile(f); } \
    PC_CHECKING { int r = ftrylockfile(f); funlockfile(f); pc_check("%d", r); }

CASE(getc_file, 1000000, 1) { GETC_BODY }
CASE(putc_devnull, 1000000, 1) { PUTC_BODY }
CASE(fputs_devnull, 1000000, 1) { FPUTS_BODY }
CASE(fgets_line_st, 1000000, 200) { FGETS_BODY }
CASE(getc_file_mt, 1000000, 1) { go_mt(); GETC_BODY }
CASE(putc_devnull_mt, 1000000, 1) { go_mt(); PUTC_BODY }
CASE(fputs_devnull_mt, 1000000, 1) { go_mt(); FPUTS_BODY }
CASE(fgets_line_mt, 1000000, 200) { go_mt(); FGETS_BODY }
CASE(flockfile_nest_mt, 100000, 1) { go_mt(); FLOCK_BODY }
