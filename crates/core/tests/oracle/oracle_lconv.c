/* argv[1] = locale: prints every struct lconv field (strings in hex, chars as numbers). */
#include <locale.h>
#include <stdio.h>
static void hex(const char *name, const char *s) {
    printf("%s=", name);
    for (const unsigned char *p = (const unsigned char *)s; *p; p++) printf("%02x", *p);
    printf("\n");
}
int main(int argc, char **argv) {
    if (argc < 2 || !setlocale(LC_ALL, argv[1])) return 1;
    struct lconv *l = localeconv();
    hex("decimal_point", l->decimal_point); hex("thousands_sep", l->thousands_sep); hex("grouping", l->grouping);
    hex("int_curr_symbol", l->int_curr_symbol); hex("currency_symbol", l->currency_symbol);
    hex("mon_decimal_point", l->mon_decimal_point); hex("mon_thousands_sep", l->mon_thousands_sep);
    hex("mon_grouping", l->mon_grouping); hex("positive_sign", l->positive_sign); hex("negative_sign", l->negative_sign);
    printf("chars=%d %d %d %d %d %d %d %d %d %d %d %d %d %d\n", l->int_frac_digits, l->frac_digits, l->p_cs_precedes,
           l->p_sep_by_space, l->n_cs_precedes, l->n_sep_by_space, l->p_sign_posn, l->n_sign_posn, l->int_p_cs_precedes,
           l->int_p_sep_by_space, l->int_n_cs_precedes, l->int_n_sep_by_space, l->int_p_sign_posn, l->int_n_sign_posn);
    return 0;
}
