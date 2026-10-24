#!/usr/bin/env python3
"""Record the reference C library's error texts as spec/abi/errmsgs.tsv (spec §2.2).

Run on the reference system (glibc 2.39, x86_64): compiles a probe that prints, for every
errno value from 0 to 133 and for an unknown one, `strerrorname_np` and `strerror` in the C
locale. The output is reference data, as spec/abi/langinfo.tsv is (D-2.2): the library embeds
the same texts and a test compares them with this file.
"""
import os
import subprocess
import sys
import tempfile

PROBE = r'''
#define _GNU_SOURCE
#include <locale.h>
#include <stdio.h>
#include <string.h>
int main(void) {
    setlocale(LC_ALL, "C");
    printf("errno\tname\tmessage\n");
    for (int e = 0; e <= 133; e++) {
        const char *n = strerrorname_np(e);
        printf("%d\t%s\t%s\n", e, n ? n : "", strerror(e));
    }
    return 0;
}
'''


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), '..', 'abi', 'errmsgs.tsv')
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, 'p.c')
        exe = os.path.join(d, 'p')
        open(src, 'w').write(PROBE)
        subprocess.check_call(['cc', '-O0', '-o', exe, src])
        text = subprocess.check_output([exe], env={'LC_ALL': 'C'}).decode()
    open(out, 'w', newline='\n').write(text)
    print(f'{out}: {text.count(chr(10)) - 1} rows')


if __name__ == '__main__':
    main()
