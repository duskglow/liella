/* argp end to end (spec §1.7; D-1.7): built against the host C library and against Liella's
 * products, the outputs must match line for line (argp_products.rs). Black-box: each case runs
 * argp_parse in a child process, through the public <argp.h> interface, and the line shows the
 * child's exit status and everything it wrote to standard output and standard error. The parser
 * callbacks print each event they receive. */
#define _GNU_SOURCE
#include <argp.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <unistd.h>
#include "tscale.h"

static char obuf[1 << 16];
static size_t used;
static void flush_out(void) { if (used) { (void)!write(1, obuf, used); used = 0; } }
static void ch(char c) { if (used == sizeof obuf) flush_out(); obuf[used++] = c; }
static void str(const char *s) { while (*s) ch(*s++); }
static void num(long v) {
    char b[24]; int n = 0; unsigned long u = v < 0 ? -(unsigned long)v : (unsigned long)v;
    if (v < 0) ch('-');
    do { b[n++] = (char)('0' + u % 10); u /= 10; } while (u);
    while (n) ch(b[--n]);
}
static void text(const char *p, size_t n) {
    ch('[');
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)p[i];
        if (c == '\n') { ch('\\'); ch('n'); }
        else if (c >= 32 && c < 127 && c != '\\') ch((char)c);
        else { const char *d = "0123456789abcdef"; ch('\\'); ch(d[c >> 4]); ch(d[c & 15]); }
    }
    ch(']');
}

const char *argp_program_version = "prog 1.0";
const char *argp_program_bug_address = "<bugs@example.org>";

/* Events go to stdout through stdio (the child flushes before exiting). */
static const char *key_name(int key) {
    static char b[32];
    switch (key) {
    case ARGP_KEY_ARG: return "ARG";
    case ARGP_KEY_ARGS: return "ARGS";
    case ARGP_KEY_END: return "END";
    case ARGP_KEY_NO_ARGS: return "NO_ARGS";
    case ARGP_KEY_INIT: return "INIT";
    case ARGP_KEY_SUCCESS: return "SUCCESS";
    case ARGP_KEY_ERROR: return "ERROR";
    case ARGP_KEY_FINI: return "FINI";
    }
    if (key > 32 && key < 127) { b[0] = (char)key; b[1] = 0; return b; }
    snprintf(b, sizeof b, "%d", key);
    return b;
}

static int fail_mode;      /* what the main parser does on END: 0 nothing, 1 argp_error, 2 argp_failure, 3 return EINVAL */
static int min_args;       /* argp_usage when fewer operands arrive */

static error_t child_parse(int key, char *arg, struct argp_state *state) {
    printf("  child %s arg=%s argnum=%u next=%d\n", key_name(key), arg ? arg : "(null)", state->arg_num, state->next);
    switch (key) {
    case 'c': case 1001: return 0;
    default: return ARGP_ERR_UNKNOWN;
    }
}

static error_t main_parse(int key, char *arg, struct argp_state *state) {
    printf("  main %s arg=%s argnum=%u next=%d\n", key_name(key), arg ? arg : "(null)", state->arg_num, state->next);
    switch (key) {
    case 'v': case 'o': case 'l': case 'x': case 'H': case 's': case 1000:
        return 0;
    case ARGP_KEY_ARG:
        if (arg && strcmp(arg, "stop") == 0) return ARGP_ERR_UNKNOWN;
        return 0;
    case ARGP_KEY_END:
        if (state->arg_num < (unsigned)min_args) argp_usage(state);
        if (fail_mode == 1) argp_error(state, "bad thing %d", 7);
        if (fail_mode == 2) argp_failure(state, 3, ENOENT, "failure %s", "here");
        if (fail_mode == 3) return EINVAL;
        if (fail_mode == 4) argp_state_help(state, stdout, ARGP_HELP_SHORT_USAGE | ARGP_HELP_SEE);
        return 0;
    default:
        return ARGP_ERR_UNKNOWN;
    }
}

static struct argp_option child_options[] = {
    { 0, 0, 0, 0, "Child options:", 5 },
    { "child", 'c', 0, 0, "A child option", 0 },
    { "child-long", 1001, "VAL", OPTION_ARG_OPTIONAL, "A long child option with an optional value", 0 },
    { 0 },
};
static struct argp child_argp = { child_options, child_parse, 0, "Child documentation.", 0, 0, 0 };
static struct argp_child children[] = { { &child_argp, 0, 0, 0 }, { 0 } };

static struct argp_option main_options[] = {
    { "verbose", 'v', 0, 0, "Produce verbose output", 0 },
    { "output", 'o', "FILE", 0, "Output to FILE instead of standard output, with a description long enough to wrap", 0 },
    { "level", 'l', "N", OPTION_ARG_OPTIONAL, "Set the level", 0 },
    { "quiet", 0, 0, OPTION_ALIAS, 0, 0 },
    { "silent", 's', 0, OPTION_ALIAS, 0, 0 },
    { "hidden", 'H', 0, OPTION_HIDDEN, "Hidden option", 0 },
    { 0, 1011, "ARG", OPTION_HIDDEN, "Hidden option with neither name nor printable key", 0 },
    { 0, 0, 0, 0, "Advanced options:", 2 },
    { "long-only", 1000, 0, 0, "Only a long name", 2 },
    { 0, 'x', 0, 0, "Only a short name", 2 },
    { 0, 1010, 0, 0, "Option with neither name nor printable key", 2 },
    { "FORMAT", 0, 0, OPTION_DOC | OPTION_NO_USAGE, "A documentation entry", 3 },
    { 0 },
};
static struct argp main_argp = {
    main_options, main_parse, "ARG1 ARG2\n-x ARG", "Program description.\vText after the options.", children, 0, 0,
};

/* Headers whose text is empty or only spaces, as group header entries, as keyless entries and as
 * child headers (each child in a group of its own). */
static error_t hdr_parse(int key, char *arg, struct argp_state *state) {
    (void)key; (void)arg; (void)state;
    return ARGP_ERR_UNKNOWN;
}
static struct argp_option hdr_child1_options[] = { { "c-one", 'p', 0, 0, "First child", 0 }, { 0 } };
static struct argp_option hdr_child2_options[] = { { "c-two", 'q', 0, 0, "Second child", 0 }, { 0 } };
static struct argp_option hdr_child3_options[] = { { "c-three", 'r', 0, 0, "Third child", 0 }, { 0 } };
static struct argp hdr_child1 = { hdr_child1_options, hdr_parse, 0, 0, 0, 0, 0 };
static struct argp hdr_child2 = { hdr_child2_options, hdr_parse, 0, 0, 0, 0, 0 };
static struct argp hdr_child3 = { hdr_child3_options, hdr_parse, 0, 0, 0, 0, 0 };
static struct argp_child hdr_children[] = {
    { &hdr_child1, 0, "", 8 }, { &hdr_child2, 0, " ", 9 }, { &hdr_child3, 0, "    ", 10 }, { 0 },
};
static struct argp_option hdr_options[] = {
    { "aa", 'a', 0, 0, "First", 0 },
    { 0, 0, 0, 0, "", 1 },
    { "bb", 'b', 0, 0, "Empty header", 1 },
    { 0, 0, 0, 0, " ", 2 },
    { "cc", 'c', 0, 0, "One-space header", 2 },
    { 0, 0, 0, 0, "    ", 3 },
    { "dd", 'd', 0, 0, "Four-space header", 3 },
    { 0, 1020, 0, 0, "", 4 },
    { 0, 1021, 0, 0, " ", 4 },
    { 0, 1022, 0, 0, "    ", 4 },
    { "ee", 'e', 0, 0, "After keyless entries", 4 },
    { 0, 0, 0, 0, "  Leading spaces:", 5 },
    { "ff", 'f', 0, 0, "Leading", 5 },
    { 0, 0, 0, 0, "Trailing spaces:   ", 6 },
    { "gg", 'g', 0, 0, "Trailing", 6 },
    { 0, 0, 0, 0, "Two  inner\n  \nlines  \n\nend", 7 },
    { "hh", 'h', 0, 0, "Inner", 7 },
    { 0 },
};
static struct argp hdr_argp = { hdr_options, hdr_parse, 0, "Header texts.", hdr_children, 0, 0 };

/* The order of sibling child clusters and the blank lines between them: several children per
 * group, with and without headers, nested, mixed group numbers (ARGP-HEADER-WHITESPACE-2). */
#define ORD1(var, name, key, doc) static struct argp_option var##_o[] = { { name, key, 0, 0, doc, 0 }, { 0 } }; \
    static struct argp var = { var##_o, hdr_parse, 0, 0, 0, 0, 0 };
static struct argp_option ord_root_o[] = {
    { "root", 'z', 0, 0, "Root option", 0 }, { 0, 0, 0, 0, "Root g1:", 1 }, { "rone", 'y', 0, 0, "Root group 1", 1 }, { 0 },
};
/* Three headed siblings in group 0. */
ORD1(oa1, "c-one", 'p', "First child")
ORD1(oa2, "c-two", 'q', "Second child")
ORD1(oa3, "c-three", 'r', "Third child")
static struct argp_child ord_k1[] = { { &oa1, 0, "H-one:", 0 }, { &oa2, 0, "H-two:", 0 }, { &oa3, 0, "H-three:", 0 }, { 0 } };
/* Headerless siblings in group 0 (merged) and in group 1 (clusters). */
ORD1(ob1, "zz-first", 0x101, "Child 1")
ORD1(ob2, "mm-second", 0x102, "Child 2")
ORD1(ob3, "aa-third", 0x103, "Child 3")
ORD1(ob4, "yy-g1-first", 0x104, "Child 4")
ORD1(ob5, "bb-g1-second", 0x105, "Child 5")
ORD1(ob6, "nn-g1-third", 0x106, "Child 6")
static struct argp_child ord_k2[] = {
    { &ob1, 0, 0, 0 }, { &ob2, 0, 0, 0 }, { &ob3, 0, 0, 0 }, { &ob4, 0, 0, 1 }, { &ob5, 0, 0, 1 }, { &ob6, 0, 0, 1 }, { 0 },
};
/* Mixed group numbers, interleaved. */
ORD1(od1, "g2-a", 0x111, "group 2 first")
ORD1(od2, "g1-a", 0x112, "group 1 first")
ORD1(od3, "g2-b", 0x113, "group 2 second")
ORD1(od4, "g1-b", 0x114, "group 1 second")
ORD1(od5, "g0-a", 0x115, "group 0 first")
ORD1(od6, "gm1-a", 0x116, "group -1 first")
ORD1(od7, "gm1-b", 0x117, "group -1 second")
ORD1(od8, "g0-b", 0x118, "group 0 second")
static struct argp_child ord_k3[] = {
    { &od1, 0, "D1 g2:", 2 }, { &od2, 0, "D2 g1:", 1 }, { &od3, 0, "D3 g2:", 2 }, { &od4, 0, "D4 g1:", 1 },
    { &od5, 0, "D5 g0:", 0 }, { &od6, 0, "D6 g-1:", -1 }, { &od7, 0, "D7 g-1:", -1 }, { &od8, 0, "D8 g0:", 0 }, { 0 },
};
/* Nested headed children. */
ORD1(oe11, "a-one", 0x121, "A child 1")
ORD1(oe12, "a-two", 0x122, "A child 2")
ORD1(oe21, "b-one", 0x123, "B child 1")
static struct argp_child ord_ka[] = { { &oe11, 0, "A1:", 0 }, { &oe12, 0, "A2:", 0 }, { 0 } };
static struct argp_child ord_kb[] = { { &oe21, 0, "B1:", 0 }, { 0 } };
static struct argp_option oea_o[] = { { "a-own", 0x124, 0, 0, "A own", 0 }, { 0 } };
static struct argp_option oeb_o[] = { { "b-own", 0x125, 0, 0, "B own", 0 }, { 0 } };
static struct argp oea = { oea_o, hdr_parse, 0, 0, ord_ka, 0, 0 };
static struct argp oeb = { oeb_o, hdr_parse, 0, 0, ord_kb, 0, 0 };
ORD1(oec, "c-own", 0x126, "C own")
static struct argp_child ord_k4[] = { { &oea, 0, "A:", 0 }, { &oeb, 0, "B:", 0 }, { &oec, 0, "C:", 0 }, { 0 } };
/* Headed children with groups of their own and a nested headerless child. */
static struct argp_option ofa_o[] = { { "fa-zero", 0x131, 0, 0, "FA g0", 0 }, { 0, 0, 0, 0, "FA sub:", 1 }, { "fa-one", 0x132, 0, 0, "FA g1", 1 }, { 0 } };
static struct argp_option ofb_o[] = { { "fb-zero", 0x133, 0, 0, "FB g0", 0 }, { 0, 0, 0, 0, "FB sub:", 1 }, { "fb-one", 0x134, 0, 0, "FB g1", 1 }, { 0 } };
ORD1(ofn, "fn-nested", 0x135, "nested headerless in FB")
static struct argp_child ord_kfb[] = { { &ofn, 0, 0, 0 }, { 0 } };
static struct argp ofa = { ofa_o, hdr_parse, 0, 0, 0, 0, 0 };
static struct argp ofb = { ofb_o, hdr_parse, 0, 0, ord_kfb, 0, 0 };
ORD1(ofc, "fc-hl", 0x136, "headerless group 0")
static struct argp_child ord_k5[] = { { &ofa, 0, "FA:", 0 }, { &ofc, 0, 0, 0 }, { &ofb, 0, "FB:", 0 }, { 0 } };
/* Three levels of headed children in group 1. */
ORD1(og3, "deep3", 0x141, "level 3")
static struct argp_child ord_kg2[] = { { &og3, 0, "L3:", 0 }, { 0 } };
static struct argp_option og2_o[] = { { "deep2", 0x142, 0, 0, "level 2", 0 }, { 0 } };
static struct argp og2 = { og2_o, hdr_parse, 0, 0, ord_kg2, 0, 0 };
ORD1(og2s, "deep2-sib", 0x143, "level 2 sibling")
static struct argp_child ord_kg1[] = { { &og2, 0, "L2:", 0 }, { &og2s, 0, "L2sib:", 0 }, { 0 } };
static struct argp_option og1_o[] = { { "deep1", 0x144, 0, 0, "level 1", 0 }, { 0 } };
static struct argp og1 = { og1_o, hdr_parse, 0, 0, ord_kg1, 0, 0 };
ORD1(og1s, "deep1-sib", 0x145, "level 1 sibling")
static struct argp_child ord_k6[] = { { &og1, 0, "L1:", 1 }, { &og1s, 0, "L1sib:", 1 }, { 0 } };
/* Blank child headers in group 0, a group header first in a headed child, a headerless child. */
static struct argp_option ohh_o[] = { { 0, 0, 0, 0, "Child group header:", 0 }, { "hh-opt", 0x151, 0, 0, "after a group header", 0 }, { 0 } };
static struct argp ohh = { ohh_o, hdr_parse, 0, 0, 0, 0, 0 };
ORD1(ohl, "hl-opt", 0x152, "headerless child")
static struct argp_child ord_k7[] = {
    { &hdr_child1, 0, "", 0 }, { &hdr_child2, 0, " ", 0 }, { &hdr_child3, 0, "    ", 0 }, { &ohh, 0, "HH:", 0 }, { &ohl, 0, 0, 0 }, { 0 },
};
/* A headed child without options of its own around a headed child; a headed child left for its
 * sub-child and re-entered in its group 1; a keyless entry first in a headed child; a headed child
 * inside a headerless cluster. */
ORD1(ox1, "x-one", 0x161, "X child")
static struct argp_child ord_kx[] = { { &ox1, 0, "X1:", 0 }, { 0 } };
static struct argp ox = { 0, hdr_parse, 0, 0, ord_kx, 0, 0 };
ORD1(or1x, "r-sub", 0x162, "R child")
static struct argp_child ord_kr[] = { { &or1x, 0, "R1:", 0 }, { 0 } };
static struct argp_option or_o[] = { { "r-zero", 0x163, 0, 0, "R g0", 0 }, { "r-one", 0x164, 0, 0, "R g1", 1 }, { 0 } };
static struct argp orr = { or_o, hdr_parse, 0, 0, ord_kr, 0, 0 };
static struct argp_option ok_o[] = { { 0, 1170, 0, 0, "Keyless first", 0 }, { "k-opt", 0x165, 0, 0, "K option", 0 }, { 0 } };
static struct argp ok = { ok_o, hdr_parse, 0, 0, 0, 0, 0 };
ORD1(on1, "n-sub", 0x166, "N child")
static struct argp_child ord_kn[] = { { &on1, 0, "N1:", 0 }, { 0 } };
static struct argp_option on_o[] = { { "n-own", 0x167, 0, 0, "N own", 0 }, { 0 } };
static struct argp onn = { on_o, hdr_parse, 0, 0, ord_kn, 0, 0 };
static struct argp_child ord_k8[] = { { &ox, 0, "X:", 0 }, { &orr, 0, "R:", 0 }, { &ok, 0, "K:", 0 }, { &onn, 0, 0, 2 }, { 0 } };
/* Nested headed children with mixed group numbers, and a nested child's own groups. */
ORD1(osaa, "saa-opt", 0x17a, "Saa g0")
static struct argp_child ord_ksa[] = { { &osaa, 0, "Saa:", 0 }, { 0 } };
static struct argp_option osa_o[] = { { "sa-opt", 0x171, 0, 0, "Sa g0", 0 }, { "sa-one", 0x17b, 0, 0, "Sa g1", 1 }, { 0 } };
static struct argp osa = { osa_o, hdr_parse, 0, 0, ord_ksa, 0, 0 };
ORD1(osb, "sb-opt", 0x172, "Sb g2")
ORD1(osc, "sc-opt", 0x173, "Sc g1")
ORD1(osd, "sd-opt", 0x174, "Sd g-1")
static struct argp_child ord_ks[] = { { &osa, 0, "Sa:", 0 }, { &osb, 0, "Sb:", 2 }, { &osc, 0, "Sc:", 1 }, { &osd, 0, "Sd:", -1 }, { 0 } };
static struct argp_option os_o[] = {
    { "s-zero", 0x175, 0, 0, "S g0", 0 }, { "s-one", 0x176, 0, 0, "S g1", 1 }, { "s-three", 0x177, 0, 0, "S g3", 3 },
    { "s-neg", 0x178, 0, 0, "S g-1", -1 }, { 0 },
};
static struct argp os = { os_o, hdr_parse, 0, 0, ord_ks, 0, 0 };
ORD1(ot, "t-opt", 0x179, "T g1")
static struct argp_child ord_k9[] = { { &os, 0, "S:", 0 }, { &ot, 0, "T:", 1 }, { 0 } };
static struct argp ord_argp[] = {
    { ord_root_o, hdr_parse, 0, "Order 1.", ord_k1, 0, 0 }, { ord_root_o, hdr_parse, 0, "Order 2.", ord_k2, 0, 0 },
    { ord_root_o, hdr_parse, 0, "Order 3.", ord_k3, 0, 0 }, { ord_root_o, hdr_parse, 0, "Order 4.", ord_k4, 0, 0 },
    { ord_root_o, hdr_parse, 0, "Order 5.", ord_k5, 0, 0 }, { ord_root_o, hdr_parse, 0, "Order 6.", ord_k6, 0, 0 },
    { ord_root_o, hdr_parse, 0, "Order 7.", ord_k7, 0, 0 }, { ord_root_o, hdr_parse, 0, "Order 8.", ord_k8, 0, 0 },
    { ord_root_o, hdr_parse, 0, "Order 9.", ord_k9, 0, 0 },
};
/* A help filter that shows the key it is called with: it prefixes the text with "{key}" (the
 * ARGP_KEY_HELP_* keys in hex), so the help output shows which key each piece of text arrives
 * with (ARGP-HELP-FILTER-KEYS). A NULL text stays NULL. */
static char *tag_filter(int key, const char *t, void *input) {
    (void)input;
    if (!t) return 0;
    char k[32];
    if (key >= 0x2000000) snprintf(k, sizeof k, "{%#x}", (unsigned)key);
    else snprintf(k, sizeof k, "{%s}", key_name(key));
    size_t n = strlen(k), m = strlen(t);
    char *r = malloc(n + m + 1);
    if (!r) return 0;
    memcpy(r, k, n);
    memcpy(r + n, t, m + 1);
    return r;
}
static struct argp_option flt_child_o[] = {
    { 0, 0, 0, 0, "Child group header:", 0 }, { "fc", 'c', 0, 0, "Option C", 0 }, { 0 },
};
static struct argp flt_child = { flt_child_o, hdr_parse, 0, "Child pre.\vChild post.", 0, tag_filter, 0 };
static struct argp_child flt_children[] = { { &flt_child, 0, "Child header:", 2 }, { 0 } };
static struct argp_option flt_o[] = {
    { "fa", 'a', 0, 0, "Option A", 0 }, { 0, 0, 0, 0, "Group header:", 1 }, { "fb", 'b', "B", 0, "Option B", 1 },
    { "fbb", 0, 0, OPTION_ALIAS, 0, 1 }, { 0, 1180, 0, 0, "Keyless entry", 1 }, { 0 },
};
static struct argp flt_argp = { flt_o, hdr_parse, "ARGS", "Pre doc.\vPost doc.", flt_children, tag_filter, 0 };

/* Option keys ':' ';' '-' (and an ordinary 'a'): short options or not, in parsing and in help
 * (ARGP-HELP-FILTER-KEYS). ':' comes first in the table. */
static error_t keys_parse(int key, char *arg, struct argp_state *state) {
    switch (key) {
    case ':': case ';': case '-': case 'a': case ARGP_KEY_ARG:
        printf("  keys %s arg=%s next=%d\n", key_name(key), arg ? arg : "(null)", state->next);
        /* The help without the standard options (run with ARGP_NO_HELP). */
        if (key == ARGP_KEY_ARG && strcmp(arg, "help") == 0) argp_state_help(state, stdout, ARGP_HELP_STD_HELP & ~ARGP_HELP_EXIT_OK);
        if (key == ARGP_KEY_ARG && strcmp(arg, "usage") == 0) argp_state_help(state, stdout, ARGP_HELP_USAGE);
        return 0;
    default:
        return ARGP_ERR_UNKNOWN;
    }
}
static struct argp_option keys_o[] = {
    { "colon", ':', 0, 0, "Colon key", 0 }, { "semi", ';', "S", 0, "Semicolon key", 0 },
    { "dash", '-', 0, 0, "Dash key", 0 }, { "aa", 'a', 0, 0, "A key", 0 }, { 0 },
};
static struct argp keys_argp = { keys_o, keys_parse, 0, "Keys.", 0, 0, 0 };
/* '-' first in the table, operands around the options. */
static struct argp_option keys_dash_o[] = { { "dash", '-', 0, 0, "Dash key", 0 }, { "aa", 'a', 0, 0, "A key", 0 }, { 0 } };
static struct argp keys_dash_argp = { keys_dash_o, keys_parse, 0, "Dash first.", 0, 0, 0 };
static struct argp_option keys_cs_o[] = { { "colon", ':', 0, 0, "Colon key", 0 }, { "semi", ';', "S", 0, "Semicolon key", 0 }, { "aa", 'a', 0, 0, "A key", 0 }, { 0 } };
static struct argp keys_cs_argp = { keys_cs_o, keys_parse, 0, "Colon and semicolon.", 0, 0, 0 };

/* Blank lines between option groups (ARGP-HELP-FILTER-KEYS-2): by tree shape, with and without
 * headers (an empty one included), documentation entries and children with or without a header. */
static error_t blank_parse(int key, char *arg, struct argp_state *state) { (void)key; (void)arg; (void)state; return ARGP_ERR_UNKNOWN; }
static struct argp_option blank_g0_o[] = { { "aa", 'a', 0, 0, "A key", 0 }, { "bb", 'b', 0, 0, "B key", 0 }, { 0 } };
static struct argp_option blank_g01_o[] = { { "aa", 'a', 0, 0, "A key", 0 }, { "bb", 'b', 0, 0, "B key", 1 }, { "ee", 'e', 0, 0, "E key", -2 }, { 0 } };
static struct argp_option blank_empty_o[] = { { 0, 0, 0, 0, "", 0 }, { "aa", 'a', 0, 0, "A key", 0 }, { "bb", 'b', 0, 0, "B key", 3 }, { 0 } };
static struct argp_option blank_doc_o[] = { { "aa", 'a', 0, 0, "A key", 0 }, { "dd", 0, 0, OPTION_DOC, "Doc entry", 2 }, { "ee", 'e', 0, 0, "E key", 3 }, { 0 } };
static struct argp_option blank_kid_o[] = { { "cc", 'c', 0, 0, "C key", 0 }, { 0 } };
static struct argp blank_kid = { blank_kid_o, blank_parse, 0, 0, 0, 0, 0 };
static struct argp_child blank_kid_h[] = { { &blank_kid, 0, "Kid:", 0 }, { 0 } };
static struct argp_child blank_kid_e[] = { { &blank_kid, 0, "", 0 }, { 0 } };
static struct argp_child blank_kid_n[] = { { &blank_kid, 0, 0, 0 }, { 0 } };
static struct argp blank_g0 = { blank_g0_o, blank_parse, 0, "Group 0.", 0, 0, 0 };
static struct argp blank_g01 = { blank_g01_o, blank_parse, 0, "Groups.", 0, 0, 0 };
static struct argp blank_empty = { blank_empty_o, blank_parse, 0, "Empty header.", 0, 0, 0 };
static struct argp blank_doc = { blank_doc_o, blank_parse, 0, "Doc entry.", 0, 0, 0 };
static struct argp blank_kid_header = { blank_g01_o, blank_parse, 0, "Child header.", blank_kid_h, 0, 0 };
static struct argp blank_kid_empty = { blank_g0_o, blank_parse, 0, "Child empty header.", blank_kid_e, 0, 0 };
static struct argp blank_kid_none = { blank_g0_o, blank_parse, 0, "Child no header.", blank_kid_n, 0, 0 };

static const struct argp *cur_argp = &main_argp;

/* Run one parse in a child: the flags, the environment entry, and the arguments. */
static void run(const char *label, unsigned flags, const char *env, int fmode, int mins, const char *const *args) {
    flush_out();
    fflush(stdout);
    int out[2], err[2];
    (void)!pipe(out);
    (void)!pipe(err);
    pid_t pid = fork();
    if (pid == 0) {
        dup2(out[1], 1); dup2(err[1], 2);
        close(out[0]); close(err[0]);
        close(0);
        tw_alarm(5);
        if (env) putenv((char *)env);
        fail_mode = fmode;
        min_args = mins;
        char *argv[24];
        int argc = 0;
        argv[argc++] = (char *)"prog";
        for (; args[argc - 1]; argc++) argv[argc] = (char *)args[argc - 1];
        argv[argc] = 0;
        int index = -1;
        error_t r = argp_parse(cur_argp, argc, argv, flags, &index, 0);
        printf("  -> %d index=%d\n", r, index);
        fflush(stdout);
        _exit(77);
    }
    close(out[1]); close(err[1]);
    char ob[8192], eb[8192];
    size_t on = 0, en = 0;
    ssize_t k;
    while (on < sizeof ob && (k = read(out[0], ob + on, sizeof ob - on)) > 0) on += (size_t)k;
    while (en < sizeof eb && (k = read(err[0], eb + en, sizeof eb - en)) > 0) en += (size_t)k;
    close(out[0]); close(err[0]);
    int st;
    syscall(SYS_wait4, pid, &st, 0, NULL);
    str("== "); str(label); str(" exit="); num((st & 0x7f) == 0 ? (st >> 8) & 0xff : -1); ch('\n');
    str("stdout="); text(ob, on); ch('\n');
    str("stderr="); text(eb, en); ch('\n');
}

#define A(...) ((const char *const[]){ __VA_ARGS__, NULL })
#define NONE ((const char *const[]){ NULL })

int main(void) {
    program_invocation_name = (char *)"/usr/bin/prog";
    program_invocation_short_name = (char *)"prog";
    run("none", 0, 0, 0, 0, NONE);
    run("operands", 0, 0, 0, 0, A("a", "b"));
    run("short", 0, 0, 0, 0, A("-v", "-o", "f", "-of", "-l", "-l3", "-x", "-s", "-H"));
    run("long", 0, 0, 0, 0, A("--verbose", "--output=f", "--output", "g", "--level", "--level=3", "--quiet", "--long-only"));
    run("abbreviations", 0, 0, 0, 0, A("--verb", "--out=f", "--lev=2", "--lo"));
    run("ambiguous", 0, 0, 0, 0, A("--l"));
    run("child", 0, 0, 0, 0, A("-c", "--child", "--child-long", "--child-long=5", "a"));
    run("unknown short", 0, 0, 0, 0, A("-z"));
    run("unknown long", 0, 0, 0, 0, A("--unknown"));
    run("missing argument", 0, 0, 0, 0, A("-o"));
    run("unexpected argument", 0, 0, 0, 0, A("--verbose=1"));
    run("double dash", 0, 0, 0, 0, A("-v", "--", "-x", "a"));
    run("operand stops", 0, 0, 0, 0, A("a", "stop", "-v", "b"));
    run("in order", ARGP_IN_ORDER, 0, 0, 0, A("a", "-v", "b"));
    run("permuted", 0, 0, 0, 0, A("a", "-v", "b", "-x"));
    run("parse argv0", ARGP_PARSE_ARGV0, 0, 0, 0, A("-v"));
    run("too few", 0, 0, 0, 2, A("a"));
    run("argp_error", 0, 0, 1, 0, NONE);
    run("argp_failure", 0, 0, 2, 0, NONE);
    run("parser error", 0, 0, 3, 0, NONE);
    run("state help", 0, 0, 4, 0, NONE);
    run("no exit", ARGP_NO_EXIT, 0, 1, 0, NONE);
    run("no errs", ARGP_NO_ERRS, 0, 0, 0, A("-z"));
    run("no errs unknown long", ARGP_NO_ERRS, 0, 0, 0, A("--unknown"));
    run("silent", ARGP_SILENT, 0, 0, 0, A("-z"));
    run("no help", ARGP_NO_HELP, 0, 0, 0, A("--help"));
    run("help", 0, 0, 0, 0, A("--help"));
    run("help short", 0, 0, 0, 0, A("-?"));
    run("usage", 0, 0, 0, 0, A("--usage"));
    run("version", 0, 0, 0, 0, A("--version"));
    run("version short", 0, 0, 0, 0, A("-V"));
    run("program name", 0, 0, 0, 0, A("--program-name=other", "-z"));
    run("help rmargin 40", 0, "ARGP_HELP_FMT=rmargin=40", 0, 0, A("--help"));
    run("help no-dup-args", 0, "ARGP_HELP_FMT=no-dup-args", 0, 0, A("--help"));
    run("help columns", 0, "ARGP_HELP_FMT=short-opt-col=4,long-opt-col=10,opt-doc-col=20,header-col=2", 0, 0, A("--help"));
    run("help unknown fmt name", 0, "ARGP_HELP_FMT=bogus", 0, 0, A("--help"));
    run("help bad fmt value", 0, "ARGP_HELP_FMT=rmargin=x", 0, 0, A("--help"));
    run("usage rmargin 30", 0, "ARGP_HELP_FMT=rmargin=30", 0, 0, A("--usage"));
    run("help COLUMNS", 0, "COLUMNS=50", 0, 0, A("--help"));
    cur_argp = &hdr_argp;
    run("help blank headers", 0, 0, 0, 0, A("--help"));
    run("help blank headers header-col 0", 0, "ARGP_HELP_FMT=header-col=0", 0, 0, A("--help"));
    run("help blank headers header-col 4", 0, "ARGP_HELP_FMT=header-col=4", 0, 0, A("--help"));
    run("usage blank headers", 0, 0, 0, 0, A("--usage"));
    for (unsigned i = 0; i < sizeof ord_argp / sizeof *ord_argp; i++) {
        cur_argp = &ord_argp[i];
        run("help cluster order", 0, 0, 0, 0, A("--help"));
    }
    cur_argp = &flt_argp;
    run("help filter keys", 0, 0, 0, 0, A("--help"));
    run("usage filter keys", 0, 0, 0, 0, A("--usage"));
    cur_argp = &keys_argp;
    run("keys short", 0, 0, 0, 0, A("-:", "-;x", "-a-", "-;", "y", "-:a;z", "op"));
    run("keys long", 0, 0, 0, 0, A("--colon", "--semi=z", "--dash", "-", "--", "-:"));
    run("keys dash cluster end", 0, 0, 0, 0, A("-a-", "-:-"));
    run("keys missing argument", 0, 0, 0, 0, A("-;"));
    run("keys unknown", 0, 0, 0, 0, A("-z"));
    run("keys help", ARGP_NO_HELP, 0, 0, 0, A("help"));
    run("keys usage", ARGP_NO_HELP, 0, 0, 0, A("usage"));
    cur_argp = &keys_dash_argp;
    run("keys dash first", 0, 0, 0, 0, A("x", "-a-", "y", "--dash", "-z"));
    run("keys dash first help", ARGP_NO_HELP, 0, 0, 0, A("help"));
    cur_argp = &keys_cs_argp;
    run("keys colon semi help", ARGP_NO_HELP, 0, 0, 0, A("help", "usage"));
    run("keys full help", 0, 0, 0, 0, A("--help"));
    cur_argp = &blank_g0;
    run("blank group 0", 0, 0, 0, 0, A("--help"));
    cur_argp = &blank_g01;
    run("blank groups", 0, 0, 0, 0, A("--help"));
    cur_argp = &blank_empty;
    run("blank empty header", 0, 0, 0, 0, A("--help"));
    cur_argp = &blank_doc;
    run("blank doc entry", 0, 0, 0, 0, A("--help"));
    cur_argp = &blank_kid_header;
    run("blank child header", 0, 0, 0, 0, A("--help"));
    cur_argp = &blank_kid_empty;
    run("blank child empty header", 0, 0, 0, 0, A("--help"));
    cur_argp = &blank_kid_none;
    run("blank child no header", 0, 0, 0, 0, A("--help"));
    flush_out();
    return 0;
}
