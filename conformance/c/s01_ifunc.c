/* Spec §1.1 (constructors): a static program's own indirect functions (GNU ifunc,
 * R_X86_64_IRELATIVE in .rela.iplt) are resolved at start, once each, before .preinit_array and
 * the constructors run, so code there and in main calls the chosen implementation. A resolver may
 * call the library (its CPU paths are set up by then); it gets no defined arguments (glibc 2.39
 * on x86_64 passes none, probe-settled), so the two-parameter resolver below checks nothing about
 * them. The same program passes against glibc 2.39 (static link).
 * Each failing CHECK exits with its line number. */
#define _GNU_SOURCE
#include <string.h>
#include <sys/auxv.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

typedef int fn_t(int);

static int resolved_f, resolved_g, seen_preinit, seen_ctor;
static int preinit_result, ctor_result;
static unsigned long hwcap_in_resolver;
static char copied[16];

static int f_slow(int x) { return x + 1; }
static int f_fast(int x) { return x + 41; }
static int g_impl(int x) { return x * 2; }

/* Calls the library: the copy and the auxiliary vector are usable when resolvers run. */
static fn_t *resolve_f(void)
{
    resolved_f++;
    memcpy(copied, "resolver-copy", 14);
    hwcap_in_resolver = getauxval(AT_HWCAP);
    return (seen_preinit || seen_ctor) ? f_slow : f_fast;
}

/* Declared with the parameters other ELF platforms define; their values are not checked. */
static fn_t *resolve_g(unsigned long a, unsigned long b)
{
    (void)a;
    (void)b;
    resolved_g++;
    return g_impl;
}

int f(int) __attribute__((ifunc("resolve_f")));
int g(int) __attribute__((ifunc("resolve_g")));

static void preinit(int argc, char **argv, char **envp)
{
    (void)argc; (void)argv; (void)envp;
    seen_preinit = 1;
    preinit_result = f(1);
}
__attribute__((section(".preinit_array"), used)) static void (*preinit_p)(int, char **, char **) = preinit;

__attribute__((constructor)) static void ctor(void)
{
    seen_ctor = 1;
    ctor_result = f(2) + g(3);
}

int main(void)
{
    /* Each resolver ran exactly once, before .preinit_array (it chose f_fast). */
    CHECK("1.1-c-5", resolved_f == 1);
    CHECK("1.1-c-5", resolved_g == 1);
    CHECK("1.1-c-5", preinit_result == 42);
    CHECK("1.1-c-5", ctor_result == 43 + 6);
    CHECK("1.1-c-5", f(10) == 51 && g(10) == 20);
    /* Through a function pointer (the address comes from the resolved .got slot). */
    fn_t *volatile pf = f;
    fn_t *volatile pg = g;
    CHECK("1.1-c-5", pf(0) == 41 && pg(4) == 8);
    CHECK("1.1-c-5", pf == f && pg == g);
    /* The resolver's library calls worked. */
    CHECK("1.1-c-5", memcmp(copied, "resolver-copy", 14) == 0);
    CHECK("1.1-c-5", hwcap_in_resolver == getauxval(AT_HWCAP));
    CHECK("1.1-c-5", resolved_f == 1 && resolved_g == 1);
    return 0;
}
