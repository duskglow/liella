#!/usr/bin/env python3
"""Plants for tools/secret_scan.py: a scratch repo holds one fake secret per rule, in a
commit that is then deleted again (so it is history-only), plus one decoy. Prints
CAUGHT / SURVIVED per rule; exits 1 if any plant survives or the decoy is flagged.
Usage: python3 -I tools/secret_scan_selftest.py"""
import os, subprocess, sys, tempfile

here = os.path.dirname(os.path.abspath(__file__))
# fake values are assembled at run time so this file itself carries no matchable text
B = "-----BEGIN " + "RSA PRIVATE KEY-----"
PLANTS = {
    "private-key-block": B,
    "aws-key-id": "x = AK" + "IA" + "ABCDEFGHIJKLMNOP",
    "github-token": "t = gh" + "p_" + "a" * 36,
    "slack-token": "t = xo" + "xb-" + "1234567890-abcdef",
    "google-api-key": "k = AI" + "za" + "A" * 35,
    "stripe-live": "k = sk" + "_live_" + "a" * 20,
    "jwt": "j = ey" + "J" + "a" * 10 + ".ey" + "J" + "b" * 10 + "." + "c" * 10,
    "url-credentials": "u = ht" + "tp://bob:hunter22@example.org/x",
    "secret-assignment": "pass" + "word = \"hunter2hunter2\";",
}
DECOY = "int password; // getpass(\"Password: \") and http://example.org/a:b"
d = tempfile.mkdtemp(prefix="secretscan-selftest-")
g = lambda *a: subprocess.run(["git", "-C", d, "-c", "user.name=t", "-c", "user.email=t@t", *a],
                              check=True, capture_output=True)
g("init", "-q")
for i, (rule, text) in enumerate(PLANTS.items()):
    open(os.path.join(d, "p%d.txt" % i), "w").write(text + "\n")
open(os.path.join(d, "decoy.txt"), "w").write(DECOY + "\n")
g("add", ".")
g("commit", "-qm", "plants")
for i in range(len(PLANTS)):
    os.remove(os.path.join(d, "p%d.txt" % i))
g("add", "-A")
g("commit", "-qm", "remove")
r = subprocess.run([sys.executable, "-I", os.path.join(here, "secret_scan.py"), d], capture_output=True, text=True)
rows = [l.split("\t") for l in r.stdout.splitlines()]
bad = 0
for i, rule in enumerate(PLANTS):
    ok = any(x[0] == rule and x[1] == "p%d.txt" % i for x in rows)
    print("%s %s" % ("CAUGHT" if ok else "SURVIVED", rule))
    bad += not ok
if any(x[1] == "decoy.txt" for x in rows):
    print("FALSE-POSITIVE decoy flagged")
    bad += 1

# allowlist checks, on a scratch worktree file set (untracked files are scanned too)
def scan(env_allow=None):
    env = dict(os.environ)
    if env_allow is not None:
        env["SECRET_SCAN_ALLOW"] = env_allow
    return subprocess.run([sys.executable, "-I", os.path.join(here, "secret_scan.py"), d],
                          capture_output=True, text=True, env=env, stdin=subprocess.DEVNULL, timeout=300)

# (1) this very file, scanned on its own with an empty allowlist: its url-credentials decoy is still CAUGHT
os.makedirs(os.path.join(d, "tools"), exist_ok=True)
with open(os.path.join(here, "secret_scan_selftest.py")) as src, open(os.path.join(d, "tools", "secret_scan_selftest.py"), "w") as dst:
    dst.write(src.read())
empty = os.path.join(d, "empty.toml")
open(empty, "w").write("")
r1 = scan(empty)
ok = any(l.startswith("url-credentials\ttools/secret_scan_selftest.py\t") for l in r1.stdout.splitlines())
print("%s allowlist-off: selftest decoy flagged" % ("CAUGHT" if ok else "SURVIVED"))
bad += not ok
# (2) with the real allowlist the same file is silent
r2 = scan()
ok = not any("tools/secret_scan_selftest.py" in l for l in r2.stdout.splitlines())
print("%s allowlist-on: selftest fixture silent" % ("OK" if ok else "FAIL"))
bad += not ok
# (3) a new fake token in a file outside the allowlist fails the scan
open(os.path.join(d, "newfile.txt"), "w").write("t = gh" + "p_" + "b" * 36 + "\n")
r3 = scan()
ok = r3.returncode == 1 and any(l.startswith("github-token\tnewfile.txt\t") for l in r3.stdout.splitlines())
print("%s new token outside allowlist fails scan" % ("CAUGHT" if ok else "SURVIVED"))
bad += not ok
print("scratch repo (remove by hand if wanted):", d)
sys.exit(1 if bad else 0)
