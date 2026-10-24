#!/usr/bin/env python3
"""Redacted secret scan of every reachable blob (all history) plus the working tree.

Usage: python3 -I tools/secret_scan.py [repo-dir]

Every distinct blob reachable from any ref is read once through `git cat-file --batch`
(equivalent to a git-grep per commit tree deduplicated by blob) and checked against the
rule table. Binary blobs (NUL byte) are skipped, like `git grep -I`. Output lines are
`rule<TAB>path<TAB>commit<TAB>line` and NEVER contain matched text. For history hits the
commit is the newest reachable commit that touches the blob at that path (found with
`git log --all --find-object` (hash only)), "worktree" marks a hit in an untracked/modified file.
Hits matching an entry of tools/secret_scan_allow.toml ([[py_allow]]: rule + path regex + reason) are dropped;
the count of dropped hits is reported on stderr. Every subprocess has stdin=DEVNULL (or a file) so it can never wait
on an inherited stdin, and the whole scan has a timeout (env SECRET_SCAN_TIMEOUT seconds, default 900; exit 2).
Exit status 1 if there are hits, 0 if none.
"""
import os, re, signal, subprocess, sys, tomllib

repo = sys.argv[1] if len(sys.argv) > 1 else "."
ALLOW_FILE = os.environ.get("SECRET_SCAN_ALLOW") or os.path.join(os.path.dirname(os.path.abspath(__file__)), "secret_scan_allow.toml")
TIMEOUT = int(os.environ.get("SECRET_SCAN_TIMEOUT", "900"))
ALLOW = []
if os.path.exists(ALLOW_FILE):
    with open(ALLOW_FILE, "rb") as fh:
        for e in tomllib.load(fh).get("py_allow", []):
            if not (e.get("rule") and e.get("path") and e.get("reason")):
                sys.exit("secret_scan: allowlist entry needs rule, path and reason: %r" % (e,))
            ALLOW.append((e["rule"], re.compile(e["path"])))
lst = None
p = None


def on_timeout(*_):
    if p is not None:
        p.kill()
    print("secret_scan: timed out after %d s" % TIMEOUT, file=sys.stderr)
    os._exit(2)


signal.signal(signal.SIGALRM, on_timeout)
signal.alarm(TIMEOUT)
RULES = [
    ("private-key-block", r"-----BEGIN ([A-Z0-9]+ )*PRIVATE KEY( BLOCK)?-----"),
    ("aws-key-id", r"\b(AKIA|ASIA)[0-9A-Z]{16}\b"),
    ("github-token", r"\b(ghp|gho|ghs|ghu|ghr)_[A-Za-z0-9]{30,}\b|\bgithub_pat_[A-Za-z0-9_]{20,}"),
    ("slack-token", r"\bxox[abpr]-[A-Za-z0-9-]{10,}"),
    ("google-api-key", r"\bAIza[0-9A-Za-z_-]{35}\b"),
    ("stripe-live", r"\b[sr]k_live_[0-9A-Za-z]{10,}"),
    ("jwt", r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}"),
    ("url-credentials", r"://[^\s/:@'\"<>]{1,100}:[^\s/:@'\"<>]{1,100}@[^\s/'\"<>]+"),
    ("secret-assignment",
     r"(?i)\b(password|passwd|secret|token|api_?key)\b['\"]?\s*[:=]\s*['\"][^'\"\s$%{}<>()]{6,}['\"]"),
]
RX = [(n, re.compile(p.encode())) for n, p in RULES]


def git(*a, inp=None):
    return subprocess.run(["git", "-C", repo, *a], input=inp, stdin=None if inp is not None else subprocess.DEVNULL,
                          capture_output=True, check=True, timeout=TIMEOUT).stdout


def scan(data):
    """yield (rule, lineno) for each rule/line hit; text is never kept."""
    if b"\0" in data:
        return
    for lineno, line in enumerate(data.split(b"\n"), 1):
        if len(line) > 4000:
            line = line[:4000]
        if len(line) < 12:
            continue
        for n, rx in RX:
            if rx.search(line):
                yield n, lineno


# 1. history: every reachable blob once, with one path
paths = {}
for ent in git("rev-list", "--objects", "--all").decode("utf-8", "replace").splitlines():
    sha, _, path = ent.partition(" ")
    if path:
        paths.setdefault(sha, path)
types = git("cat-file", "--batch-check=%(objectname) %(objecttype)", inp="\n".join(paths).encode() + b"\n")
blobs = [l.split()[0].decode() for l in types.splitlines() if l.endswith(b" blob")]
import tempfile
lst = tempfile.TemporaryFile()
lst.write(("\n".join(blobs) + "\n").encode())
lst.seek(0)
p = subprocess.Popen(["git", "-C", repo, "cat-file", "--batch"], stdin=lst, stdout=subprocess.PIPE)
hits = []
for _ in blobs:
    hdr = p.stdout.readline().split()
    data = p.stdout.read(int(hdr[2]))
    p.stdout.read(1)
    for rule, ln in scan(data):
        hits.append((rule, paths[hdr[0].decode()], hdr[0].decode(), ln))
p.wait()
out = []
for rule, path, blob, ln in sorted(set(hits)):
    c = git("log", "--all", "--format=%h", "--find-object=" + blob, "-1").decode().strip()[:10]
    out.append((rule, path, c or "?", ln))

# 2. working tree (tracked + untracked, not ignored)
wt = git("ls-files", "-z", "--cached", "--others", "--exclude-standard").decode("utf-8", "replace").split("\0")
for f in filter(None, wt):
    fp = os.path.join(repo, f)
    try:
        if os.path.islink(fp) or not os.path.isfile(fp):
            continue
        data = open(fp, "rb").read()
    except OSError:
        continue
    for rule, ln in scan(data):
        out.append((rule, f, "worktree", ln))

n0 = len(set(out))
out = [h for h in out if not any(h[0] == r and rx.search(h[1]) for r, rx in ALLOW)]
print("# allowlisted hits dropped: %d" % (n0 - len(set(out))), file=sys.stderr)
for r in sorted(set(out)):
    print("%s\t%s\t%s\t%d" % r)
print("# blobs scanned: %d, hits: %d" % (len(blobs), len(set(out))), file=sys.stderr)
sys.exit(1 if out else 0)
