/* LIBM-RETHINK prototype per call: the library's exp against the prototype's three quick-phase variants (L plain
 * products, D Dekker's product, F one FMA), 4096 operands uniform in [-10, 10] (the cbench_libm exp range) cycled
 * through a volatile table, so the measured average includes each variant's fallback rate. Run with
 * validation/wl_percall.sh --spec <this file>; PROTO_DIR is the directory holding proto_exp.c on the host.
 * no spec clause (measurement probe). */
#define __STDC_WANT_IEC_60559_TYPES_EXT__ 1
#include "percall.h"
#ifdef PROTO_EXP /* -DPROTO_EXP='"<dir>/proto_exp.c"' on the host */
#include PROTO_EXP
#else
#include "proto_exp.c"
#endif

static volatile double ex[4096];

static void efill(void) {
    uint64_t s = 0x9e3779b97f4a7c15ull;
    for (int i = 0; i < 4096; i++) {
        s ^= s << 13; s ^= s >> 7; s ^= s << 17;
        ex[i] = -10 + 20 * ((double)(s >> 11) * 0x1p-53);
    }
}

#define E1(NAME, FN) CASE(NAME, 20000, 1) { efill(); proto_exp_init(); double acc = 0; \
    for (long i = 0; i < n; i++) acc += FN(ex[i & 4095]); pc_sink += (long)acc; \
    PC_CHECKING { pc_check("%a", FN(ex[5])); } }

E1(exp_lib, exp)
E1(exp_proto_L, proto_exp_L)
E1(exp_proto_D, proto_exp_D)
E1(exp_proto_F, proto_exp_F)
