/* Spec §27.5, D-27.6: __x86_get_cpuid_feature_leaf through the installed <sys/platform/x86.h>.
 * Each index 0..9 holds the raw CPUID words of its leaf (zero past the CPU's maximum leaf or
 * subleaf), compared here with the compiler's <cpuid.h>; an active bit is present (the one
 * exception: RTM_ALWAYS_ABORT, leaf 7.EDX bit 11, may be active without being present); AVX
 * and AVX-512F are active exactly when present and the OS enabled their XCR0 state; SSE2 is
 * present and active; indices 10, 11 and 0xffffffff share one all-zero entry, never NULL. */
#include <cpuid.h>
#include <stdio.h>
#include <string.h>
#include <sys/platform/x86.h>

static int fails;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "s27_cpuid_feature_leaf: FAIL line %d: %s\n", __LINE__, #c); fails++; } } while (0)

static const unsigned leaf_of[10][2] = {
    {1, 0}, {7, 0}, {0x80000001u, 0}, {0xd, 1}, {0x80000007u, 0}, {0x80000008u, 0}, {7, 1}, {0x19, 0}, {0x14, 0}, {0x24, 0},
};

static const unsigned *words(unsigned index)
{
    return (const unsigned *)(const void *)__x86_get_cpuid_feature_leaf(index);
}

static unsigned long long xcr0(void)
{
    unsigned a, b, c, d;
    __cpuid(1, a, b, c, d);
    if (!(c & (1u << 27))) /* OSXSAVE */
        return 0;
    unsigned lo, hi;
    __asm__ volatile("xgetbv" : "=a"(lo), "=d"(hi) : "c"(0));
    return ((unsigned long long)hi << 32) | lo;
}

int main(void)
{
    unsigned max = __get_cpuid_max(0, 0), max_ext = __get_cpuid_max(0x80000000u, 0);
    unsigned a, b, c, d, sub7_max = 0;
    if (max >= 7) {
        __cpuid_count(7, 0, a, b, c, d);
        sub7_max = a;
    }
    for (unsigned i = 0; i < 10; i++) {
        const unsigned *w = words(i);
        CHECK(w != NULL);
        if (w == NULL)
            continue;
        unsigned leaf = leaf_of[i][0], sub = leaf_of[i][1];
        int listed = leaf <= (leaf >= 0x80000000u ? max_ext : max) && (leaf != 7 || sub <= sub7_max);
        unsigned want[4] = {0, 0, 0, 0};
        if (listed)
            __cpuid_count(leaf, sub, want[0], want[1], want[2], want[3]);
        if (i == 0) /* leaf 1 EBX bits 31..24: the APIC ID of the core that ran CPUID */
            want[1] &= 0x00ffffffu;
        unsigned got1 = i == 0 ? w[1] & 0x00ffffffu : w[1];
        if (w[0] != want[0] || got1 != want[1] || w[2] != want[2] || w[3] != want[3]) {
            fprintf(stderr, "s27_cpuid_feature_leaf: index %u leaf %#x.%u: got %08x %08x %08x %08x want %08x %08x %08x %08x\n",
                    i, leaf, sub, w[0], got1, w[2], w[3], want[0], want[1], want[2], want[3]);
            fails++;
        }
        for (unsigned r = 0; r < 4; r++) {
            unsigned extra = w[4 + r] & ~w[r];
            if (i == 1 && r == 3)
                extra &= ~(1u << 11); /* RTM_ALWAYS_ABORT */
            if (extra) {
                fprintf(stderr, "s27_cpuid_feature_leaf: index %u word %u: active bits %08x not present\n", i, r, extra);
                fails++;
            }
        }
    }

    CHECK(CPU_FEATURE_PRESENT(SSE2));
    CHECK(CPU_FEATURE_ACTIVE(SSE2));
    unsigned long long x = xcr0();
    CHECK(!!CPU_FEATURE_ACTIVE(AVX) == (CPU_FEATURE_PRESENT(AVX) && (x & 0x6) == 0x6));
    CHECK(!!CPU_FEATURE_ACTIVE(AVX2) == (CPU_FEATURE_PRESENT(AVX2) && (x & 0x6) == 0x6));
    CHECK(!!CPU_FEATURE_ACTIVE(AVX512F) == (CPU_FEATURE_PRESENT(AVX512F) && (x & 0xe6) == 0xe6));

    static const unsigned char zero[32];
    const unsigned *z = words(10);
    CHECK(z != NULL);
    CHECK(words(11) == z);
    CHECK(words(0xffffffffu) == z);
    CHECK(z == NULL || memcmp(z, zero, sizeof zero) == 0);
    CHECK(z == NULL || !CPU_FEATURE_PRESENT(SSE2) || z != words(0));

    if (fails)
        return 1;
    printf("s27_cpuid_feature_leaf: ok\n");
    return 0;
}
