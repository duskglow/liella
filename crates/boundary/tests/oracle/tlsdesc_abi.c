/* Oracle for crates/boundary/tests/ld_tlsdesc.rs: the register convention of the TLS descriptor
 * functions in csys/src/machine/tlsdesc.S (DL-15 `DL-TLS-THREADS`, R14-8; A. Oliva, "Thread-Local
 * Storage Descriptors for IA32 and AMD64/EM64T"). Linked with that file and tlsdesc_harness.S.
 *
 * Each function is called directly (the variant is chosen here, not by a loader), with the stack
 * aligned and misaligned by 8 at the call, and with every general register, xmm0-15, MXCSR and the
 * x87 control word in a known state. After the call %rax must hold the expected offset from the
 * thread pointer and everything else must be unchanged. The dynamic function is tried with a
 * resolver that changes every caller-saved register and the floating-point controls, and with a
 * plain C resolver. Prints one line per difference; exits 0 when there is none. */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

struct state {
    uint64_t g[13]; /* rbx rcx rdx rsi rdi r8 r9 r10 r11 r12 r13 r14 r15 */
    uint64_t rax;
    unsigned char xmm[16][16];
    uint32_t mxcsr;
    uint16_t fcw;
    uint16_t pad;
};
_Static_assert(offsetof(struct state, rax) == 104, "harness layout");
_Static_assert(offsetof(struct state, xmm) == 112, "harness layout");
_Static_assert(offsetof(struct state, mxcsr) == 368, "harness layout");
_Static_assert(offsetof(struct state, fcw) == 372, "harness layout");

extern void liella_machine_ld_tlsdesc_static(void);
extern void liella_machine_ld_tlsdesc_dynamic(void);
extern void tlsdesc_call(const uintptr_t *desc, const struct state *in, struct state *out, long misalign);
extern uintptr_t clobber_resolver(const uintptr_t *rec);

/* A resolver as the loader's would be: ordinary compiled code. */
static uintptr_t c_resolver(const uintptr_t *rec) {
    volatile double d = (double)rec[1] * 1.5;
    (void)d;
    return rec[1];
}

static uintptr_t thread_pointer(void) {
    uintptr_t v;
    __asm__ volatile("movq %%fs:0, %0" : "=r"(v));
    return v;
}

static const char *const names[13] = {"rbx", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"};

static int compare(const char *what, long mis, const struct state *in, const struct state *out, uint64_t want) {
    int bad = 0;
    for (int i = 0; i < 13; i++) {
        if (in->g[i] != out->g[i]) {
            printf("%s (stack off by %ld): %s changed: %#llx -> %#llx\n", what, mis, names[i], (unsigned long long)in->g[i], (unsigned long long)out->g[i]);
            bad = 1;
        }
    }
    if (out->rax != want) {
        printf("%s (stack off by %ld): rax %#llx, want %#llx\n", what, mis, (unsigned long long)out->rax, (unsigned long long)want);
        bad = 1;
    }
    for (int i = 0; i < 16; i++) {
        if (memcmp(in->xmm[i], out->xmm[i], 16) != 0) {
            printf("%s (stack off by %ld): xmm%d changed\n", what, mis, i);
            bad = 1;
        }
    }
    if (in->mxcsr != out->mxcsr) {
        printf("%s (stack off by %ld): mxcsr changed: %#x -> %#x\n", what, mis, in->mxcsr, out->mxcsr);
        bad = 1;
    }
    if (in->fcw != out->fcw) {
        printf("%s (stack off by %ld): x87 control word changed: %#x -> %#x\n", what, mis, in->fcw, out->fcw);
        bad = 1;
    }
    return bad;
}

int main(void) {
    struct state in;
    memset(&in, 0, sizeof in);
    for (int i = 0; i < 13; i++) {
        in.g[i] = 0x0101010101010101ULL * (uint64_t)(i + 1) ^ 0x8000000000000000ULL;
    }
    for (int i = 0; i < 16; i++) {
        for (int j = 0; j < 16; j++) {
            in.xmm[i][j] = (unsigned char)(i * 16 + j + 1);
        }
    }
    in.mxcsr = 0x3f80; /* round down, every exception masked */
    in.fcw = 0x0b7f;   /* round up, extended precision */
    int bad = 0;
    for (long mis = 0; mis <= 8; mis += 8) {
        struct state out;
        uintptr_t off = (uintptr_t)-0x1238;
        const uintptr_t sdesc[2] = {(uintptr_t)liella_machine_ld_tlsdesc_static, off};
        memset(&out, 0, sizeof out);
        tlsdesc_call(sdesc, &in, &out, mis);
        bad |= compare("static", mis, &in, &out, off);

        uintptr_t tp = thread_pointer();
        const uintptr_t rec1[2] = {(uintptr_t)clobber_resolver, tp - 0x40};
        const uintptr_t ddesc1[2] = {(uintptr_t)liella_machine_ld_tlsdesc_dynamic, (uintptr_t)rec1};
        memset(&out, 0, sizeof out);
        tlsdesc_call(ddesc1, &in, &out, mis);
        bad |= compare("dynamic, clobbering resolver", mis, &in, &out, (uintptr_t)-0x40);

        const uintptr_t rec2[2] = {(uintptr_t)c_resolver, tp + 0x88};
        const uintptr_t ddesc2[2] = {(uintptr_t)liella_machine_ld_tlsdesc_dynamic, (uintptr_t)rec2};
        memset(&out, 0, sizeof out);
        tlsdesc_call(ddesc2, &in, &out, mis);
        bad |= compare("dynamic, C resolver", mis, &in, &out, 0x88);
    }
    if (!bad) {
        puts("tlsdesc: ok");
    }
    return bad;
}
