/* iconv end to end (spec §6.4; D-6.8, D-6.9): built against the host C library and against
 * Liella's products, the outputs must match line for line (iconv_products.rs). Each line is
 * one conversion: the return value, errno, the input left and the output in hex. A conversion
 * is also run one input byte at a time, which must give the same output (resumption). */
#include <errno.h>
#include <iconv.h>
#include <locale.h>
#include <string.h>
#include <unistd.h>

/* stdio is not used (Liella's streams are a later phase): output goes through write(2). */
static char obuf[1 << 16];
static size_t used;

static void flush(void) { if (used) { (void)!write(1, obuf, used); used = 0; } }
static void ch(char c) { if (used == sizeof obuf) flush(); obuf[used++] = c; }
static void str(const char *s) { while (*s) ch(*s++); }
static void hexb(unsigned char b) { const char *d = "0123456789abcdef"; ch(d[b >> 4]); ch(d[b & 15]); }
static void num(long v) {
    char b[24]; int n = 0; unsigned long u = v < 0 ? -(unsigned long)v : (unsigned long)v;
    if (v < 0) ch('-');
    do { b[n++] = (char)('0' + u % 10); u /= 10; } while (u);
    while (n) ch(b[--n]);
}
static void hex(const unsigned char *p, size_t n) { for (size_t i = 0; i < n; i++) hexb(p[i]); }

static void conv(const char *to, const char *from, const void *in, size_t n, size_t room) {
    str(to); str(" <- "); str(from); str(" [");
    hex(in, n);
    str("] ");
    iconv_t cd = iconv_open(to, from);
    if (cd == (iconv_t)-1) { str("open errno "); num(errno); ch('\n'); return; }
    unsigned char out[256]; char *ip = (char *)in, *op = (char *)out; size_t il = n, ol = room;
    errno = 0;
    size_t r = iconv(cd, &ip, &il, &op, &ol);
    str("r="); num((long)r); str(" errno="); num(r == (size_t)-1 ? errno : 0); str(" left="); num((long)il); str(" out=");
    hex(out, room - ol);
    /* Again on the same descriptor (no BOM the second time), then after a flush. */
    ip = (char *)in; il = n; op = (char *)out; ol = room;
    iconv(cd, &ip, &il, &op, &ol);
    str(" again=");
    hex(out, room - ol);
    op = (char *)out; ol = room;
    size_t f = iconv(cd, NULL, NULL, &op, &ol);
    str(" flush="); num((long)f);
    ip = (char *)in; il = n; op = (char *)out; ol = room;
    iconv(cd, &ip, &il, &op, &ol);
    str(" after=");
    hex(out, room - ol);
    /* One byte at a time: an incomplete character waits for the next byte. */
    iconv(cd, NULL, NULL, NULL, NULL);
    unsigned char out2[256]; char *op2 = (char *)out2; size_t ol2 = room, pos = 0;
    long rr = 0; int err = 0;
    for (size_t fed = 1; fed <= n; fed++) {
        char *ip2 = (char *)in + pos; size_t il2 = fed - pos;
        errno = 0;
        size_t q = iconv(cd, &ip2, &il2, &op2, &ol2);
        pos = fed - il2;
        if (q == (size_t)-1 && errno != EINVAL) { err = errno; break; }
        if (q != (size_t)-1) rr += (long)q;
    }
    str(" bytewise=");
    hex(out2, room - ol2);
    str(" r="); num(rr); str(" err="); num(err); str(" left="); num((long)(n - pos));
    ch('\n');
    iconv_close(cd);
}

int main(void) {
    const char a[] = "A\xc3\xa9\xf0\x9f\x98\x80"; /* A é U+1F600 */
    const char *uni[] = { "UTF-16", "UTF-16LE", "UTF-16BE", "UTF-32", "UTF-32LE", "UTF-32BE", "UCS-2", "UCS-2LE", "UCS-2BE",
                          "UNICODELITTLE", "UNICODEBIG", "UCS-4", "UCS-4LE", "UCS-4BE", "WCHAR_T", "UTF8", "utf-8", "UCS2",
                          "UCS4", "UTF16", "UTF32", "ISO-10646/UCS2", "ISO-10646/UCS4", "ISO-10646/UTF8", "10646-1:1993",
                          "ASCII", "ISO-8859-1", "LATIN1", "l1", "us-ascii", "ANSI_X3.4-1968", NULL };
    for (int i = 0; uni[i]; i++) conv(uni[i], "UTF-8", a, sizeof a - 1, 64);
    conv("UTF-16", "UTF-8", a, sizeof a - 1, 1);
    conv("UTF-16", "UTF-8", a, sizeof a - 1, 3);
    conv("UTF-32", "UTF-8", "", 0, 64);

    const unsigned char be16[] = { 0xfe, 0xff, 0, 0x41 }, le16[] = { 0xff, 0xfe, 0x41, 0 }, no16[] = { 0, 0x41, 0, 0x42 },
                        sur[] = { 0xd8, 0x3d, 0xde, 0x00 }, lsur[] = { 0x3d, 0xd8, 0x00, 0xde }, lone[] = { 0xdc, 0x00, 0, 0x41 },
                        half[] = { 0xd8, 0x3d }, odd[] = { 0, 0x41, 0 };
    const char *dec16[] = { "UTF-16", "UTF-16BE", "UTF-16LE", "UCS-2", "UCS-2BE", "UCS-2LE", NULL };
    struct { const unsigned char *p; size_t n; } in16[] = { { be16, 4 }, { le16, 4 }, { no16, 4 }, { sur, 4 }, { lsur, 4 }, { lone, 4 }, { half, 2 }, { odd, 3 } };
    for (int d = 0; dec16[d]; d++)
        for (unsigned k = 0; k < sizeof in16 / sizeof in16[0]; k++) conv("UTF-8", dec16[d], in16[k].p, in16[k].n, 64);

    const unsigned char be32[] = { 0, 0, 0xfe, 0xff, 0, 0, 0, 0x41 }, le32[] = { 0xff, 0xfe, 0, 0, 0x41, 0, 0, 0 }, no32[] = { 0, 0, 0, 0x41 },
                        big[] = { 0, 0x11, 0, 0 }, s32[] = { 0, 0, 0xd8, 0 }, neg[] = { 0x80, 0, 0, 0 };
    const char *dec32[] = { "UTF-32", "UTF-32BE", "UTF-32LE", "UCS-4", "UCS-4LE", "WCHAR_T", NULL };
    struct { const unsigned char *p; size_t n; } in32[] = { { be32, 8 }, { le32, 8 }, { no32, 4 }, { big, 4 }, { s32, 4 }, { neg, 4 }, { no32, 3 } };
    for (int d = 0; dec32[d]; d++)
        for (unsigned k = 0; k < sizeof in32 / sizeof in32[0]; k++) conv("UTF-8", dec32[d], in32[k].p, in32[k].n, 64);
    const char *enc31[] = { "UTF-16LE", "UTF-32LE", "UCS-4LE", "ISO-8859-1", "UTF-8", NULL };
    for (int d = 0; enc31[d]; d++) conv(enc31[d], "UCS-4", big, 4, 64);
    conv("UCS-2", "UTF-8", "\xf0\x9f\x98\x80" "A", 5, 64);

    const char *bad8[] = { "a\xc3", "a\xff" "b", "\xed\xa0\x80", "\xc0\xaf", "\xf4\x90\x80\x80", "\xe2\x82", NULL };
    for (int k = 0; bad8[k]; k++) conv("UTF-16LE", "UTF-8", bad8[k], strlen(bad8[k]), 64);

    /* Charmap charsets, both directions, and their aliases. */
    const char jp[] = "a\xe3\x81\x82\xef\xbd\xb1\xe4\xb8\x80\xe2\x80\x94z"; /* a あ ｱ 一 — z */
    const char *cms[] = { "EUC-JP", "eucjp", "SHIFT_JIS", "SJIS", "CP932", "ISO-2022-JP", "GB18030", "BIG5", "EUC-KR", "KOI8-R", "KOI8R",
                          "CP1252", "WINDOWS-1252", "ISO-8859-15", "ISO885915", "LATIN-9", "TIS-620", "IBM037", "UTF-7", "NO-SUCH", NULL };
    for (int k = 0; cms[k]; k++) conv(cms[k], "UTF-8", jp, sizeof jp - 1, 64);
    const unsigned char eucjp[] = { 'a', 0xa4, 0xa2, 0x8e, 0xb1, 0xb0, 0xec, 0x8f, 0xb0, 0xa1, 'z' }, eucbad[] = { 'a', 0xa4, 0x20 }, euccut[] = { 'a', 0x8f, 0xb0 };
    conv("UTF-8", "EUC-JP", eucjp, sizeof eucjp, 64);
    conv("UTF-8", "EUC-JP", eucbad, sizeof eucbad, 64);
    conv("UTF-8", "EUC-JP", euccut, sizeof euccut, 64);
    conv("UTF-16LE", "EUC-JP", eucjp, sizeof eucjp, 64);
    const unsigned char koi[] = { 0xc1, 0xc2, 0xd7, 0x41 }, cp[] = { 0x80, 0x81, 0x9f, 0xe9 };
    conv("UTF-8", "KOI8-R", koi, sizeof koi, 64);
    conv("UTF-8", "CP1252", cp, sizeof cp, 64);
    conv("EUC-JP", "SHIFT_JIS", "\x82\xa0\x88\xea", 4, 64);

    /* Unencodable characters: plain, //IGNORE, //TRANSLIT, in several locales. */
    const char t[] = "x\xc3\xa9\xe2\x82\xac\xc5\x93\xe2\x80\x9cq\xe2\x80\x9d\xe4\xb8\x80y";
    const char u[] = "\xc3\xa4\xc3\x9f\xe2\x80\x93\xc2\xbd\xef\xac\x81\xc2\xa9\xc2\xab\xe2\x80\xa6" "e\xcc\x81";
    const char *tos[] = { "ASCII", "ASCII//IGNORE", "ASCII//TRANSLIT", "ASCII//TRANSLIT//IGNORE", "ASCII//TRANSLIT,IGNORE",
                          "ASCII//IGNORE//TRANSLIT", "ISO-8859-1//TRANSLIT", "ascii//translit", "ASCII//", "ASCII//BOGUS", "EUC-JP//TRANSLIT", NULL };
    const char *locs[] = { "C", "C.UTF-8", "en_US.UTF-8", "de_DE.UTF-8", NULL };
    for (int l = 0; locs[l]; l++) {
        str("locale "); str(setlocale(LC_ALL, locs[l]) ? locs[l] : "(failed)"); ch('\n');
        for (int k = 0; tos[k]; k++) {
            conv(tos[k], "UTF-8", t, sizeof t - 1, 64);
            conv(tos[k], "UTF-8", u, sizeof u - 1, 64);
        }
        conv("ASCII//IGNORE", "UTF-8", "x\xff\xc3\xa9y", 5, 64);
        conv("ASCII//IGNORE", "UTF-8", "x\xe4\xb8\x80yz", 6, 2);
        conv("ASCII//IGNORE", "UTF-8", "x\xe4\xb8\x80y\xc3", 6, 64);
        conv("ASCII", "UTF-8//IGNORE", "x\xc3\xa9y", 4, 64);
        conv("", "UTF-8", "a\xc3\xa9", 3, 64);
        conv("UTF-8", "", "a\xe9", 2, 64);
    }
    setlocale(LC_ALL, "C");
    flush();
    return 0;
}
