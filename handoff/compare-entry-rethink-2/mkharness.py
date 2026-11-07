#!/usr/bin/env python3
"""Derive harness2.c (COMPARE-ENTRY-RETHINK-2) from the first rethink's harness.c by exact replacements.
usage: mkharness.py <in harness.c> <out harness2.c>"""
import sys
src = open(sys.argv[1]).read()
R = [
    ("extern int proto_memcmp(const void *, const void *, size_t);\n"
     "extern int proto_memeq(const void *, const void *, size_t);\n"
     "extern int proto_null(const void *, const void *, size_t);\n"
     "extern unsigned char proto_use_avx2;\n",
     "#define P(x) extern int x(const void *, const void *, size_t);\n"
     "P(p_memcmp_sse2) P(p_memeq_sse2) P(p_memcmp_avx2) P(p_memeq_avx2) P(p_memcmp_avx2b) P(p_memeq_avx2b)\n"
     "P(p_memcmp_ptr) P(p_memeq_ptr) P(proto_null)\n"
     "extern cmp_fn p_memcmp_slot, p_memeq_slot;\n"
     "unsigned char proto_use_avx2;\n"
     "static int HAVE_AVX2;\n"),
    ("struct fn { const char *name; cmp_fn f; int ordered; int proto; };\n",
     "struct fn { const char *name; cmp_fn f; int ordered; int proto; int avx2; };\n"),
    ('    {"proto_memcmp", proto_memcmp, 1, 1}, {"proto_memeq", proto_memeq, 0, 1},\n',
     '    {"p_memcmp_sse2", p_memcmp_sse2, 1, 1, 0}, {"p_memeq_sse2", p_memeq_sse2, 0, 1, 0},\n'
     '    {"p_memcmp_ptr", p_memcmp_ptr, 1, 1, 0}, {"p_memeq_ptr", p_memeq_ptr, 0, 1, 0},\n'
     '    {"p_memcmp_avx2", p_memcmp_avx2, 1, 1, 1}, {"p_memeq_avx2", p_memeq_avx2, 0, 1, 1},\n'
     '    {"p_memcmp_avx2b", p_memcmp_avx2b, 1, 1, 1}, {"p_memeq_avx2b", p_memeq_avx2b, 0, 1, 1},\n'),
    ("    for (int k = 0; k < NFN; k++) {\n        int v = FNS[k].f(a, b, n);",
     "    for (int k = 0; k < NFN; k++) {\n        if (FNS[k].avx2 && !HAVE_AVX2) continue;\n"
     "        int v = FNS[k].f(a, b, n);"),
    ("    proto_use_avx2 = (unsigned char)(avx2 && !(pe && pe[0] == '0'));\n",
     "    proto_use_avx2 = (unsigned char)(avx2 && !(pe && pe[0] == '0'));\n"
     "    HAVE_AVX2 = avx2;\n"
     "    p_memcmp_slot = proto_use_avx2 ? p_memcmp_avx2 : p_memcmp_sse2;\n"
     "    p_memeq_slot = proto_use_avx2 ? p_memeq_avx2 : p_memeq_sse2;\n"),
    ("        if (avx2) { unsigned char s = proto_use_avx2; proto_use_avx2 = !s; check_all(); proto_use_avx2 = s; }\n",
     ""),
    ("    static const size_t sizes[] = {8, 16, 32, 64, 96, 127, 128, 144, 256, 1024, 4096};\n",
     "    static const size_t sizes[] = {4, 8, 12, 16, 24, 32, 48, 64, 96, 127, 128, 144, 256, 1024, 4096};\n"
     "    const int NSZ = (int)(sizeof sizes / sizeof sizes[0]);\n"),
    ("    for (int k = 0; k < NFN; k++)\n        for (int si = 0; si < (int)(sizeof sizes / sizeof sizes[0]) + 1; si++)",
     "    for (int k = 0; k < NFN; k++)\n        for (int si = 0; si < NSZ + 1; si++)"),
    ("                size_t len = si < 11 ? sizes[si] : 0;\n                int mix = si == 11;\n",
     "                if (FNS[k].avx2 && !avx2) continue;\n"
     "                size_t len = si < NSZ ? sizes[si] : 0;\n                int mix = si == NSZ;\n"),
    ('    printf("H variant=%s mode=%s cpu_avx2=%d proto_avx2=%d tunables=%s cpu=%.48s\\n", VARIANT, mode, avx2,\n',
     '    printf("H variant=%s mode=%s cpu_avx2=%d proto_avx2=%d ptr=%s tunables=%s cpu=%.48s\\n", VARIANT, mode, avx2,\n'
     '           proto_use_avx2 ? "avx2" : "sse2",\n'),
]
for old, new in R:
    assert src.count(old) == 1, old[:70]
    src = src.replace(old, new)
src = src.replace("COMPARE-HEAD-RETHINK black-box harness", "COMPARE-ENTRY-RETHINK-2 harness (from COMPARE-HEAD-RETHINK's)", 1)
open(sys.argv[2], "w").write(src)
print("ok", len(src))
