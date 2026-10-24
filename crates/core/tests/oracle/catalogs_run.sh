#!/bin/sh
# Run catalogs_diff.c (spec §7.7 gettext family, §7.8 catopen/catgets/catclose; D-7.5, D-7.6).
# First build fixture catalogs into SCRATCH/fix with the system's own tools: msgfmt (GNU gettext,
# package `gettext`) for .mo files and gencat (libc-bin) for .cat files. The probe writes its other
# catalogs itself, below SCRATCH; nothing is written into the system's locale directories.
# The probe needs LOCPATH naming the test locales (target/test-locales).
#
# Usage: catalogs_run.sh PROGRAM SCRATCH [unstable]
# With `unstable`, the probe runs only the damaged-catalog cases left out of the record (offsets
# and sizes pointing outside the file; see catalogs.md).
set -eu
prog=$1
dir=$2
rm -rf "$dir"
mkdir -p "$dir"
dir=$(cd "$dir" && pwd -P)
trap 'chmod -R u+rwx "$dir" 2>/dev/null; rm -rf "$dir"' EXIT
fix=$dir/fix
mkdir "$fix"

if command -v msgfmt >/dev/null 2>&1; then
    echo "msgfmt: present"
else
    echo "msgfmt: missing (install the gettext package); the msgfmt fixtures are absent"
fi
if command -v gencat >/dev/null 2>&1; then
    echo "gencat: present"
else
    echo "gencat: missing; the gencat fixtures are absent"
fi

# ---- .mo fixtures
cat >"$fix/a.po" <<'EOF'
msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"
"Plural-Forms: nplurals=2; plural=n != 1;\n"

msgid "hello"
msgstr "hallo"

msgctxt "menu"
msgid "open"
msgstr "oeffnen"

msgid "file"
msgid_plural "files"
msgstr[0] "Datei"
msgstr[1] "Dateien"
EOF

{
    printf 'msgid ""\nmsgstr ""\n"Content-Type: text/plain; charset=UTF-8\\n"\n\n'
    i=0
    while [ $i -lt 500 ]; do
        n=$(printf '%03d' $i)
        printf 'msgid "key-%s"\nmsgstr "val-%s"\n\n' "$n" "$n"
        i=$((i + 1))
    done
} >"$fix/big.po"

cat >"$fix/ru.po" <<'EOF'
msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"
"Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\n"

msgid "file"
msgid_plural "files"
msgstr[0] "r0"
msgstr[1] "r1"
msgstr[2] "r2"
EOF

{
    printf 'msgid ""\nmsgstr ""\n"Content-Type: text/plain; charset=ISO-8859-1\\n"\n\n'
    printf 'msgid "latin"\nmsgstr "\351t\351"\n'
} >"$fix/latin1.po"

# A c-format message with <PRIu64>: msgfmt writes a revision-1 file with a system-dependent string.
cat >"$fix/sd.po" <<'EOF'
msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"

#, c-format
msgid "count %<PRIu64> x"
msgstr "zahl %<PRIu64> y"
EOF

if command -v msgfmt >/dev/null 2>&1; then
    msgfmt -o "$fix/sd.mo" "$fix/sd.po"
    msgfmt -o "$fix/a.mo" "$fix/a.po"
    msgfmt --no-hash -o "$fix/a-nohash.mo" "$fix/a.po"
    msgfmt --endianness=big -o "$fix/a-be.mo" "$fix/a.po"
    msgfmt -o "$fix/big.mo" "$fix/big.po"
    msgfmt --no-hash -o "$fix/big-nohash.mo" "$fix/big.po"
    msgfmt -o "$fix/ru.mo" "$fix/ru.po"
    msgfmt -o "$fix/latin1.mo" "$fix/latin1.po"
fi

# ---- .cat fixtures (gencat's input format: POSIX gencat, EXTENDED DESCRIPTION)
cat >"$fix/a.msg" <<'EOF'
$ comment line
$set 1
1 Hello set one
2 Second\tmessage\n
5 five
$set 3
1 set three msg one
7 "quoted?"
$quote "
8 "now quoted"
EOF

cat >"$fix/b.msg" <<'EOF'
1 default set msg
$set 2
3 two-three
$set 10
100 ten-hundred
EOF

printf '$set 1\n1 x\n' >"$fix/c.msg"

# Message 7 has a separator and no text, message 8 neither (a deletion); line 6 continues.
# gencat runs in C.UTF-8: in the C locale it rejects a byte outside ASCII ("invalid character:
# message ignored"), and an octal escape above \177 ("invalid escape sequence"), and exits 1.
printf '%s\n' \
    '$ Liella catalogs fixture' \
    '$set 1 first set' \
    '1 one' \
    '2 tab\there' \
    '3 newline\nend' \
    '4 back\\slash' \
    '5 octal \101\102\103 and \7 and \0101' \
    '6 continued \' \
    'line' \
    '7 ' \
    '8' \
    '9 other escapes \v\b\r\f and \q unknown' \
    "$(printf '10 high bytes \303\244 (UTF-8)')" \
    '$quote "' \
    '11 "quoted trailing space "' \
    '12 ""' \
    '13 "quote inside \" escaped"' \
    '$quote' \
    '14 "no longer quoted"' \
    '15 two  spaces' \
    '16	tab separator' \
    '17  leading space kept' \
    '32767 max message' \
    '100000 beyond NL_MSGMAX?' \
    '$set 2' \
    '1 set two one' \
    '3 set two three' \
    '$set 7' \
    '1 seven' \
    '$delset 7 remove it' \
    '$set 9' \
    '1 nine' >"$fix/main.msg"

if command -v gencat >/dev/null 2>&1; then
    for m in a b c main; do
        LC_ALL=C.UTF-8 gencat -o "$fix/$m.cat" "$fix/$m.msg" 2>"$fix/$m.err" || echo "gencat $m.msg failed"
        if [ -s "$fix/$m.err" ]; then
            echo "gencat $m.msg diagnostics:"
            sed "s|$fix|<fix>|g" "$fix/$m.err"
        fi
    done
fi

. "$(dirname "$0")/tscale.sh"
timeout $((900 * TSCALE)) "$prog" "$dir" ${3+"$3"}
