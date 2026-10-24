#!/usr/bin/env python3
"""Reduce harvest logs (validation/harvest/harvest.c) to replayable corpora.

    harvest_reduce.py OUTDIR LOGDIR...

Each LOGDIR holds harvest-<pid>.log files of one build (its label is in each file's B line).
Writes OUTDIR/regex.corpus and OUTDIR/printf.corpus: self-contained, deduplicated lines (a
regex execution carries its pattern and compile flags), each ending in a field "@" + the
comma-separated labels of the builds that made the call. At most PER_PATTERN executions per
pattern and PER_FORMAT calls per format string are kept (environment variables of those names;
defaults 200 and 50; the repository's corpora use smaller caps) (the first seen, in a stable order), so
the corpora stay small; the raw logs stay where they are. Also prints counts.

Corpus lines (tab-separated, strings escaped as in the logs):
  P  cflags pattern ret                                     regcomp
  E  cflags pattern eflags nmatch subject startend ret regs regexec
  G  syntax translate pattern message                       re_compile_pattern
  S  syntax translate pattern not_bol not_eol nl subject start range regs? ret regs   re_search
  M  syntax translate pattern not_bol not_eol nl subject start regs? ret regs         re_match
  F  locale errno format arg...                             a printf-family call
"""
import collections
import os
import sys

PER_PATTERN = int(os.environ.get('PER_PATTERN', '200'))
PER_FORMAT = int(os.environ.get('PER_FORMAT', '50'))


def main():
    out = sys.argv[1]
    regex = collections.OrderedDict()
    printf = collections.OrderedDict()
    unparsed = collections.Counter()
    per_pattern = collections.Counter()
    per_format = collections.Counter()
    files = 0
    for logdir in sys.argv[2:]:
        for name in sorted(os.listdir(logdir)):
            if not name.endswith('.log'):
                continue
            files += 1
            label = '?'
            comp = {}
            with open(os.path.join(logdir, name), encoding='latin-1') as f:
                for line in f:
                    fl = line.rstrip('\n').split('\t')
                    k = fl[0]
                    rec = None
                    key = None
                    if k == 'B' and len(fl) >= 2:
                        label = fl[1]
                    elif k == 'RC' and len(fl) == 5:
                        comp[fl[1]] = ('posix', fl[2], fl[3])
                        rec = ['P', fl[2], fl[3], fl[4]]
                    elif k == 'RE' and len(fl) == 9 and fl[1] in comp and comp[fl[1]][0] == 'posix':
                        _, cflags, pat = comp[fl[1]]
                        rec = ['E', cflags, pat] + fl[2:9]
                        key = pat
                    elif k == 'RP' and len(fl) == 6:
                        comp[fl[1]] = ('gnu', fl[2], fl[3], fl[4])
                        rec = ['G', fl[2], fl[3], fl[4], fl[5]]
                    elif k in ('RS', 'RM') and fl[1] in comp and comp[fl[1]][0] == 'gnu':
                        _, syn, tr, pat = comp[fl[1]]
                        rec = [k[1], syn, tr, pat] + fl[2:]
                        key = pat
                    elif k == 'PF' and len(fl) >= 5:
                        rec = ['F'] + fl[2:]
                        if per_format[fl[4]] >= PER_FORMAT and tuple(rec) not in printf:
                            continue
                        line_key = tuple(rec)
                        if line_key not in printf:
                            per_format[fl[4]] += 1
                            printf[line_key] = set()
                        printf[line_key].add(label)
                        continue
                    elif k == 'PU' and len(fl) >= 3:
                        unparsed[fl[2]] += 1
                        continue
                    if rec is None:
                        continue
                    t = tuple(rec)
                    if t not in regex:
                        if key is not None:
                            if per_pattern[key] >= PER_PATTERN:
                                continue
                            per_pattern[key] += 1
                        regex[t] = set()
                    regex[t].add(label)
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, 'regex.corpus'), 'w', encoding='latin-1', newline='\n') as f:
        for t, labels in regex.items():
            f.write('\t'.join(t) + '\t@' + ','.join(sorted(labels)) + '\n')
    with open(os.path.join(out, 'printf.corpus'), 'w', encoding='latin-1', newline='\n') as f:
        for t, labels in printf.items():
            f.write('\t'.join(t) + '\t@' + ','.join(sorted(labels)) + '\n')
    kinds = collections.Counter(t[0] for t in regex)
    print(f'{files} logs; regex lines {len(regex)} ({dict(kinds)}), patterns {len(per_pattern)}; '
          f'printf lines {len(printf)}, formats {len(per_format)}; unparsed formats {len(unparsed)}')
    for fmt, n in unparsed.most_common(10):
        print(f'  unparsed ({n}): {fmt[:120]}')


if __name__ == '__main__':
    main()
