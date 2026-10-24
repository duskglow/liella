/* gettext's second directory, /usr/share/locale-langpack (spec §7.7; D-7.5; owner ruling
 * 2026-10-22, #411): which catalog answers when the domain's directory and the language-pack
 * directory hold catalogs for the same domain. Run by langpack_run.sh with both directories
 * replaced by fixtures in a private user and mount namespace; each msgstr names where it lives. */
#include <libintl.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>

int main(void)
{
    static const char *doms[] = { "both", "loconly", "lponly", "none", "order", "langorder", "msgmiss", "boundstd" };
    printf("setlocale: %s\n", setlocale(LC_ALL, "") ? "ok" : "NULL");
    bindtextdomain("boundstd", "/usr/share/locale/");
    bindtextdomain("boundotherlp", getenv("OTHERDIR"));
    bindtextdomain("boundotherstd", getenv("OTHERDIR"));
    for (size_t i = 0; i < sizeof doms / sizeof *doms; i++)
        printf("dgettext %s: %s\n", doms[i], dgettext(doms[i], "hello"));
    printf("dgettext boundotherlp: %s\n", dgettext("boundotherlp", "hello"));
    printf("dgettext boundotherstd: %s\n", dgettext("boundotherstd", "hello"));
    printf("dcgettext lptime LC_TIME: %s\n", dcgettext("lptime", "hello", LC_TIME));
    printf("bindtextdomain lponly: %s\n", bindtextdomain("lponly", NULL));
    return 0;
}
