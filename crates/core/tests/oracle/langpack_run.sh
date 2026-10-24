#!/bin/sh
# Usage: langpack_run.sh <program> <scratch dir>. Builds the catalogs (msgfmt) under the scratch
# directory, then runs the program twice (LANGUAGE unset, then fr:en_US) in a private user and
# mount namespace (unprivileged: `unshare -r -m`) with a tmpfs over /usr/share holding only the
# fixture locale and locale-langpack directories. Exit 77 when msgfmt, unshare or en_US.UTF-8 is
# missing here.
set -eu
prog=$1
d=$2
command -v msgfmt >/dev/null 2>&1 || exit 77
unshare -r -m true 2>/dev/null || exit 77
locale -a 2>/dev/null | grep -qix 'en_US.utf8' || exit 77
rm -rf "$d"
mkdir -p "$d/l" "$d/lp" "$d/other"
mo() { # root language category domain msgid text
    mkdir -p "$d/$1/$2/$3"
    printf 'msgid ""\nmsgstr "Content-Type: text/plain; charset=UTF-8\\n"\nmsgid "%s"\nmsgstr "%s"\n' "$5" "$6" > "$d/t.po"
    msgfmt -o "$d/$1/$2/$3/$4.mo" "$d/t.po"
}
for dom in both loconly boundstd boundotherstd; do mo l en_US LC_MESSAGES $dom hello loc; done
for dom in both lponly boundotherlp; do mo lp en_US LC_MESSAGES $dom hello langpack; done
mo l en LC_MESSAGES order hello loc-en
mo lp en_US LC_MESSAGES order hello langpack-en_US
mo l en_US LC_MESSAGES langorder hello loc-en_US
mo lp fr LC_MESSAGES langorder hello langpack-fr
mo l en_US LC_MESSAGES msgmiss other loc-other
mo lp en_US LC_MESSAGES msgmiss hello langpack
mo lp en_US LC_TIME lptime hello langpack-time
for language in "" "fr:en_US"; do
    echo "## LANGUAGE=$language"
    env -i PATH="$PATH" LANG=en_US.UTF-8 LANGUAGE="$language" OTHERDIR="$d/other" unshare -r -m sh -c '
        mount -t tmpfs none /usr/share && mkdir /usr/share/locale /usr/share/locale-langpack &&
        mount --bind "$1/l" /usr/share/locale && mount --bind "$1/lp" /usr/share/locale-langpack && exec "$2"' sh "$d" "$prog"
done
