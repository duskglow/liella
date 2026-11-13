/* `sqrt` and `sqrtf` follow the caller's floating-point environment however it was set (D-10.101:
 * no MXCSR read on entry, the instruction runs on the caller's own word): through fesetround,
 * through a direct `ldmxcsr`, in a new thread (which starts with its creator's environment) and in
 * a signal handler; FTZ and DAZ; an unmasked exception traps in the call, before any EDOM store,
 * as the reference does (probed).
 * Clauses: s10_0_f_0 (the root is correctly rounded in the caller's direction with its flags and
 * errno), s10_3_c_r3 (`sqrt` is correctly rounded, `sqrt(-0)` = -0).
 * Self-checking: each failed check prints `FAIL line N: <check>`; exit 1 on any failure.
 * The functions are called through volatile pointers so the compiler cannot expand them inline. */
/* LINK-LIBM: yes */
#define _GNU_SOURCE
#include <errno.h>
#include <fenv.h>
#include <math.h>
#include <pthread.h>
#include <setjmp.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>

static double (*volatile dsq)(double) = sqrt;
static float (*volatile fsq)(float) = sqrtf;
static volatile int failed;

#define CHECK(c) do { if (!(c)) { fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #c); failed = 1; } } while (0)

static unsigned get_mx(void) { unsigned v; __asm__ volatile("stmxcsr %0" : "=m"(v)); return v; }
static void set_mx(unsigned v) { __asm__ volatile("ldmxcsr %0" : : "m"(v)); }

/* The sign of r*r - x, exactly: fma in round to nearest keeps the sign of its exact result. */
static int side(double r, double x)
{
    unsigned mx = get_mx();
    set_mx(0x1f80);
    double d = fma(r, r, -x);
    set_mx(mx);
    return (d > 0) - (d < 0);
}

/* Whether r is sqrt(x) rounded in direction rc (MXCSR RC field 0..3: nearest, down, up, zero),
 * for x > 0 and r a double (dbl) or the float root widened (flt). */
static int rounded_ok(double r, double x, unsigned rc, int flt)
{
    double lo = flt ? (double)nextafterf((float)r, 0.0f) : nextafter(r, 0.0);
    double hi = flt ? (double)nextafterf((float)r, INFINITY) : nextafter(r, INFINITY);
    int s = side(r, x);
    if (s == 0)
        return 1;
    if (rc == 1 || rc == 3)
        return s < 0 && side(hi, x) > 0;
    if (rc == 2)
        return s > 0 && side(lo, x) < 0;
    /* nearest: the exact root lies within half a unit; compare the two candidates' squares */
    double other = s > 0 ? lo : hi;
    double e1 = fabs(fma(r, r, -x)), e2 = fabs(fma(other, other, -x));
    return side(other, x) != s && e1 <= e2;
}

static const double xs[] = { 2.0, 3.0, 5.0, 0.1, 1e300, 7e-300, 12345.678 };

/* Every operand in the environment now in force, whose RC field is rc. Returns the failures. */
static int check_env(unsigned rc)
{
    int bad = 0;
    for (unsigned i = 0; i < sizeof xs / sizeof xs[0]; i++) {
        double x = xs[i];
        if (!rounded_ok(dsq(x), x, rc, 0)) {
            fprintf(stderr, "sqrt(%a) rc %u wrong\n", x, rc);
            bad++;
        }
        float xf = (float)x;
        if (xf != 0 && xf != INFINITY && !rounded_ok(fsq(xf), xf, rc, 1)) {
            fprintf(stderr, "sqrtf(%a) rc %u wrong\n", (double)xf, rc);
            bad++;
        }
    }
    return bad;
}

static const int fe_dir[4] = { FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO };

static void *thread_body(void *arg)
{
    unsigned rc = *(unsigned *)arg;
    /* the new thread starts in its creator's direction */
    CHECK(fegetround() == fe_dir[rc]);
    CHECK(check_env(rc) == 0);
    /* and its own changes are its own */
    CHECK(fesetround(FE_DOWNWARD) == 0);
    CHECK(check_env(1) == 0);
    return 0;
}

static volatile int handler_bad = -1;

static void on_usr1(int sig)
{
    (void)sig;
    unsigned saved = get_mx();
    int bad = 0;
    for (unsigned rc = 0; rc < 4; rc++) {
        fesetround(fe_dir[rc]);
        bad += check_env(rc);
    }
    set_mx(saved);
    handler_bad = bad;
}

static sigjmp_buf jb;
static volatile int errno_at_trap;

static void on_fpe(int sig, siginfo_t *si, void *uc)
{
    (void)sig; (void)uc;
    errno_at_trap = errno;
    siglongjmp(jb, si->si_code);
}

/* Call sqrt (f32 = 0) or sqrtf (1) on x under the MXCSR word mx: the si_code of a trap, or 0. */
static int trap_code(unsigned mx, int f32, double x, double *out)
{
    errno = 0;
    errno_at_trap = -1;
    int code = sigsetjmp(jb, 1);
    if (code != 0) {
        set_mx(0x1f80);
        return code;
    }
    set_mx(mx);
    *out = f32 ? (double)fsq((float)x) : dsq(x);
    set_mx(0x1f80);
    return 0;
}

int main(void)
{
    unsigned base = get_mx();
    /* 1. fesetround */
    for (unsigned rc = 0; rc < 4; rc++) {
        CHECK(fesetround(fe_dir[rc]) == 0);
        CHECK(check_env(rc) == 0);
    }
    /* 2. a direct ldmxcsr, with FTZ and DAZ in every combination */
    for (unsigned ctl = 0; ctl < 4; ctl++) {
        for (unsigned rc = 0; rc < 4; rc++) {
            unsigned w = 0x1f80 | (rc << 13) | (ctl & 1 ? 0x8000u : 0) | (ctl & 2 ? 0x40u : 0);
            set_mx(w);
            CHECK(check_env(rc) == 0);
            /* a subnormal operand: its exact root, or a zero of its sign under DAZ, no EDOM */
            errno = 0;
            double r = dsq(0x1p-1060);
            double n = dsq(-0x1p-1060);
            float rf = fsq(0x1p-140f);
            float nf = fsq(-0x1p-140f);
            int e = errno;
            set_mx(base);
            if (ctl & 2) {
                CHECK(r == 0 && !signbit(r) && rf == 0 && !signbit(rf));
                CHECK(n == 0 && signbit(n) && nf == 0 && signbit(nf));
                CHECK(e == 0);
            } else {
                CHECK(r == 0x1p-530 && rf == 0x1p-70f);
                CHECK(isnan(n) && isnan(nf) && e == EDOM);
            }
        }
    }
    set_mx(base);
    /* 3. a new thread, created in each direction */
    for (unsigned rc = 0; rc < 4; rc++) {
        CHECK(fesetround(fe_dir[rc]) == 0);
        pthread_t t;
        unsigned arg = rc;
        CHECK(pthread_create(&t, 0, thread_body, &arg) == 0);
        CHECK(pthread_join(t, 0) == 0);
        CHECK(fegetround() == fe_dir[rc]);
        CHECK(check_env(rc) == 0);
    }
    /* 4. a signal handler that changes the direction, interrupting code in another one */
    CHECK(fesetround(FE_UPWARD) == 0);
    signal(SIGUSR1, on_usr1);
    raise(SIGUSR1);
    CHECK(handler_bad == 0);
    CHECK(fegetround() == FE_UPWARD);
    CHECK(check_env(2) == 0);
    CHECK(fesetround(FE_TONEAREST) == 0);
    /* 5. unmasked exceptions trap in the call, as the reference's do (probed), before any EDOM
     * store; the masks a square root never raises change nothing */
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_sigaction = on_fpe;
    sa.sa_flags = SA_SIGINFO | SA_NODEFER;
    CHECK(sigaction(SIGFPE, &sa, 0) == 0);
    for (int f = 0; f < 2; f++) {
        double out = 0;
        double sub = f ? 0x1p-140 : 0x1p-1060;
        CHECK(trap_code(0x1f80 & ~0x80u, f, -1.0, &out) == FPE_FLTINV && errno_at_trap == 0);
        CHECK(trap_code((0x1f80 & ~0x80u) | 0x4000, f, -INFINITY, &out) == FPE_FLTINV && errno_at_trap == 0);
        CHECK(trap_code(0x1f80 & ~0x1000u, f, 2.0, &out) == FPE_FLTRES);
        CHECK(trap_code(0x1f80 & ~0x1000u, f, 4.0, &out) == 0 && out == 2.0);
        CHECK(trap_code(0x1f80 & ~0x80u, f, -0.0, &out) == 0 && out == 0 && signbit(out) && errno == 0);
        CHECK(trap_code((0x1f80 & ~0x80u) | 0x40, f, -sub, &out) == 0 && out == 0 && signbit(out) && errno == 0);
        CHECK(trap_code(0x1f80 & ~0x600u, f, -1.0, &out) == 0 && isnan(out) && errno == EDOM);
        if (!f) {
            CHECK(trap_code(0x1f80 & ~0x100u, f, sub, &out) != 0);
            CHECK(trap_code(0x1f80 & ~0x800u, f, sub, &out) == 0 && out == 0x1p-530);
        }
        CHECK(trap_code(0x1f80 & ~0x100u, f, 1.0, &out) == 0 && out == 1.0);
    }
    set_mx(base);
    if (failed)
        return 1;
    puts("ok");
    return 0;
}
