/* memcpy, memmove and memset from a constructor, before main, and again in main (D-4.25).
 *
 * The small-copy kernels pick their CPU build (SSE2 or AVX2) once at startup; a call that comes
 * before that decision runs the baseline build. Either way every byte must be right: each size
 * 0 to 256 at several alignments and overlaps, against a byte loop. The same source also serves
 * as the dynamic check against libc.so.6 (liella-historic:handoff/mem-ifunc.md). */
#include <string.h>

#define CHECK(clause, cond) do { if (!(cond)) return __LINE__; } while (0)

static unsigned char buf[1024], want[1024];

static void pattern(int seed)
{
    for (int i = 0; i < (int) sizeof buf; i++)
        buf[i] = want[i] = (unsigned char) (i * 131 + seed * 7 + 1);
}

static void oracle_move(int d, int s, int n)
{
    unsigned char tmp[256];
    for (int i = 0; i < n; i++)
        tmp[i] = want[s + i];
    for (int i = 0; i < n; i++)
        want[d + i] = tmp[i];
}

/* 0 when every size, alignment and overlap matches the byte loop; else a line number. */
static int sweep(void)
{
    static const int shifts[] = { 0, 1, -1, 7, -7, 16, -16, 31, -31, 33, -33, 300, -300 };
    for (int n = 0; n <= 256; n++) {
        for (int a = 0; a < 4; a++) {
            for (unsigned k = 0; k < sizeof shifts / sizeof shifts[0]; k++) {
                int s = 400 + a * 5, d = s + shifts[k];
                pattern(n + a);
                oracle_move(d, s, n);
                void *r = (k & 1) ? memmove(buf + d, buf + s, (size_t) n) : memcpy(buf + d, buf + s, (size_t) n);
                CHECK("s4_2_c_1", r == buf + d);
                CHECK("s4_2_c_1", memcmp(buf, want, sizeof buf) == 0);
            }
            pattern(n);
            for (int i = 0; i < n; i++)
                want[300 + a + i] = 0xC3;
            CHECK("s4_1_c_1", memset(buf + 300 + a, 0xC3, (size_t) n) == buf + 300 + a);
            CHECK("s4_1_c_1", memcmp(buf, want, sizeof buf) == 0);
        }
    }
    return 0;
}

static int early = -1;

__attribute__((constructor)) static void before_main(void)
{
    early = sweep();
}

int main(void)
{
    CHECK("s4_2_c_1", early == 0);
    return sweep();
}
