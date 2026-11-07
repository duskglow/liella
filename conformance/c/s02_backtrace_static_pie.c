/* Spec §2.7 (D-2.7, D-27.12): backtrace, backtrace_symbols and backtrace_symbols_fd in a static
 * PIE started from liella-rcrt1.o, which the kernel loads at a non-zero load bias. The same source
 * built as a fixed-address static program (bias 0) must pass the same checks; the clause tests in
 * crates/core/tests/s02_diagnostics_clauses.rs build it both ways, and conformance/run.sh builds
 * it as the static PIE its directive names. The builder names the link kind in the first
 * argument, `pie` or `fixed` (the preprocessor cannot tell: a compiler that builds -fPIE by
 * default defines __PIE__ in a fixed-address link too); the program checks that the file is of
 * that kind (ET_DYN or ET_EXEC) and that the load bias is non-zero or zero. Self-checking: each
 * clause ends with `end <clause>` on standard error, each failed check prints `FAIL <clause>:
 * line N: <check>`.
 *
 * The bias is found independently of the library: AT_ENTRY minus the e_entry of the file
 * /proc/self/exe. The image's runtime extent is its PT_LOAD segments (read from the same file)
 * moved by that bias. The program's own functions sit in sections the linker lays out in name
 * order (`.text.sorted.*`), so a return address is named by the function containing it. */
/* LINK-STATIC-PIE: relr */
/* RUN-ARGS: pie */
#define _GNU_SOURCE
#include <elf.h>
#include <execinfo.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <unistd.h>

static int failures;
static const char *current;

static void begin(const char *clause) { current = clause; }
static void end(void) { fprintf(stderr, "end %s\n", current); }

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

#define PLACED(n) __attribute__((noinline, noclone, section(".text.sorted.liella_btpie_" #n)))

static void *trace[64];
static int count;

PLACED(01) int leaf(void) { count = backtrace(trace, 64); return count + 1; }
PLACED(02) int f2(void) { return leaf() + 1; }
PLACED(03) int f1(void) { return f2() + 1; }
PLACED(04) int driver(void) { return f1() + 1; }
PLACED(05) int placed_end(void) { return 0; }

static void *const order[] = { (void *)leaf, (void *)f2, (void *)f1, (void *)driver, (void *)placed_end };
static const char *const names[] = { "leaf", "f2", "f1", "driver" };
enum { KNOWN = 4 };

/* The program function containing the return address `x`, or "~". */
static const char *name_of(uintptr_t x)
{
    for (unsigned i = 0; i + 1 < sizeof order / sizeof order[0]; i++)
        if (x > (uintptr_t)order[i] && x <= (uintptr_t)order[i + 1])
            return names[i];
    return "~";
}

/* The image's link-time extent [lo, hi), entry point and ELF type, from the file. 0 on success. */
static int image(uintptr_t *lo, uintptr_t *hi, uintptr_t *entry, unsigned *type)
{
    int fd = open("/proc/self/exe", O_RDONLY);
    if (fd < 0)
        return -1;
    Elf64_Ehdr eh;
    int rc = -1;
    if (pread(fd, &eh, sizeof eh, 0) == (ssize_t)sizeof eh && eh.e_phentsize == sizeof(Elf64_Phdr)) {
        *lo = UINTPTR_MAX;
        *hi = 0;
        *entry = eh.e_entry;
        *type = eh.e_type;
        for (unsigned i = 0; i < eh.e_phnum; i++) {
            Elf64_Phdr ph;
            if (pread(fd, &ph, sizeof ph, (off_t)(eh.e_phoff + i * sizeof ph)) != (ssize_t)sizeof ph)
                break;
            if (ph.p_type != PT_LOAD)
                continue;
            if (ph.p_vaddr < *lo)
                *lo = ph.p_vaddr;
            if (ph.p_vaddr + ph.p_memsz > *hi)
                *hi = ph.p_vaddr + ph.p_memsz;
            rc = 0;
        }
    }
    close(fd);
    return rc;
}

/* The offset in a text "prog(+0x...)": the number after "(+", or UINTPTR_MAX. */
static uintptr_t offset_in(const char *text)
{
    const char *p = strstr(text, "(+");
    return p ? (uintptr_t)strtoull(p + 2, NULL, 16) : UINTPTR_MAX;
}

int main(int argc, char **argv)
{
    int pie = argc > 1 && strcmp(argv[1], "pie") == 0;
    if (argc != 2 || (!pie && strcmp(argv[1], "fixed") != 0)) {
        fprintf(stderr, "the link kind (pie or fixed) is not the only argument: the checks cannot run\n");
        return 1;
    }
    uintptr_t lo, hi, link_entry;
    unsigned type;
    if (image(&lo, &hi, &link_entry, &type) != 0) {
        fprintf(stderr, "/proc/self/exe cannot be read: the checks cannot run\n");
        return 1;
    }
    int layout_ok = 1;
    for (unsigned i = 0; i + 1 < sizeof order / sizeof order[0]; i++)
        layout_ok &= (uintptr_t)order[i] < (uintptr_t)order[i + 1];
    if (!layout_ok) {
        fprintf(stderr, "the functions are not laid out in order: the checks cannot run\n");
        return 1;
    }
    uintptr_t bias = (uintptr_t)getauxval(AT_ENTRY) - link_entry;
    uintptr_t start = lo + bias, stop = hi + bias;
    fprintf(stderr, "bias %#lx, image [%#lx, %#lx)\n", (unsigned long)bias, (unsigned long)start, (unsigned long)stop);

    begin("s2_7_c_1");
    if (pie) {
        CHECK(type == ET_DYN);
        CHECK(bias != 0); /* the kernel loads a static PIE at a non-zero bias */
    } else {
        CHECK(type == ET_EXEC);
        CHECK(bias == 0);
    }
    CHECK((uintptr_t)driver >= start && (uintptr_t)driver < stop);
    driver();
    int full = count;
    CHECK(full >= KNOWN + 1); /* leaf f2 f1 driver main ... */
    /* Every return address lies in the loaded image (the library is linked into it), and the
     * innermost are the program's known functions, innermost first. */
    for (int i = 0; i < full; i++)
        CHECK((uintptr_t)trace[i] >= start && (uintptr_t)trace[i] < stop);
    for (int i = 0; i < KNOWN && i < full; i++)
        if (strcmp(name_of((uintptr_t)trace[i]), names[i]) != 0) {
            failures++;
            fprintf(stderr, "FAIL s2_7_c_1: frame %d is [%s], not [%s]\n", i, name_of((uintptr_t)trace[i]), names[i]);
        }
    end();

    begin("s2_7_c_2");
    {
        char **s = backtrace_symbols(trace, full);
        CHECK(s != NULL);
        for (int i = 0; s && i < full; i++) {
            /* `program(+offset from the load bias) [address]` (D-2.7). */
            char want[256];
            snprintf(want, sizeof want, "%s(+%p) [%p]", argv[0], (void *)((uintptr_t)trace[i] - bias), trace[i]);
            if (strcmp(s[i], want) != 0) {
                failures++;
                fprintf(stderr, "FAIL s2_7_c_2: frame %d\n  got  [%s]\n  want [%s]\n", i, s[i], want);
            }
            /* The offset is the link-time address: moved by the bias it names the known function. */
            if (i < KNOWN && strcmp(name_of(offset_in(s[i]) + bias), names[i]) != 0) {
                failures++;
                fprintf(stderr, "FAIL s2_7_c_2: [%s] does not name %s\n", s[i], names[i]);
            }
        }
        free(s);
    }
    end();

    begin("s2_7_c_3");
    {
        FILE *f = tmpfile();
        CHECK(f != NULL);
        if (f) {
            backtrace_symbols_fd(trace, full, fileno(f));
            static char got[8192], want[8192];
            ssize_t n = pread(fileno(f), got, sizeof got - 1, 0);
            got[n > 0 ? n : 0] = 0;
            size_t k = 0;
            for (int i = 0; i < full && k < sizeof want; i++)
                k += (size_t)snprintf(want + k, sizeof want - k, "%s(+%p)[%p]\n", argv[0],
                    (void *)((uintptr_t)trace[i] - bias), trace[i]);
            if (strcmp(got, want) != 0) {
                failures++;
                fprintf(stderr, "FAIL s2_7_c_3: written text\n  got  [%s]\n  want [%s]\n", got, want);
            }
            CHECK(strcmp(name_of(offset_in(got) + bias), names[0]) == 0);
            fclose(f);
        }
    }
    end();

    return failures != 0;
}
