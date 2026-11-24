#!/usr/bin/env python3
"""The third-party Rust crates in tessera-ui and tessera-events, with each
one's licence and its copyright lines, as Markdown: what MIT, BSD and
Apache-2.0 ask to travel with binary copies (audit #202). Read from the
Cargo.lock files and the crates' own folders in cargo's registry (so run it
where the programs were built: the release builder, or a machine that built
them).

    tools/rust-licences.py [--out FILE]     the report (stdout without --out)
    tools/rust-licences.py --check          exit 1 when a crate's sources
                                            aren't in the registry here

The release builder puts it in each bundle as share/licenses/tessera/
THIRD-PARTY-RUST.md; THIRD_PARTY_NOTICES.md points to it.
"""
import argparse
import glob
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOCKS = ("native/tessera-ui/Cargo.lock", "native/tessera-events/Cargo.lock")
OURS = re.compile(r"^tessera[-_]")
LICENCE_FILES = ("LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "LICENSE.md", "LICENSE.txt", "COPYING", "LICENSE-BSD", "LICENSE-ZLIB")


def locked(path):
    """[(name, version)] of the registry packages in a Cargo.lock"""
    out, name, version, source = [], None, None, None
    with open(path) as f:
        for line in f.read().splitlines() + ["[[package]]"]:
            if line.strip() == "[[package]]":
                if name and version and source and source.startswith('"registry+'):
                    out.append((name, version))
                name = version = source = None
                continue
            k, _, v = line.partition(" = ")
            if k == "name":
                name = v.strip('"')
            elif k == "version":
                version = v.strip('"')
            elif k == "source":
                source = v
    return out


def crate_dir(name, version):
    home = os.environ.get("CARGO_HOME") or os.path.expanduser("~/.cargo")
    for d in glob.glob(os.path.join(home, "registry", "src", "*", f"{name}-{version}")):
        return d
    vendored = os.path.join(ROOT, "native/tessera-ui/vendor", name)
    return vendored if os.path.isdir(vendored) else None


def licence_of(d):
    try:
        with open(os.path.join(d, "Cargo.toml")) as f:
            text = f.read()
    except OSError:
        return ""
    m = re.search(r'^license\s*=\s*"([^"]+)"', text, re.M)
    if m:
        return m.group(1)
    m = re.search(r'^license-file\s*=\s*"([^"]+)"', text, re.M)
    return f"see {m.group(1)}" if m else ""


def copyrights(d):
    """The crate's own copyright lines (from its licence files)"""
    lines = []
    for n in LICENCE_FILES:
        try:
            with open(os.path.join(d, n), errors="replace") as f:
                for line in f:
                    s = line.strip()
                    # (real ones: "Copyright (c) 2019 …", "© 2020 …"; never the licence's own words)
                    if re.match(r"^(copyright\s*(\(c\)|©)?\s*\d{4}|\(c\)\s*\d{4}|©\s*\d{4})", s, re.I) and s not in lines:
                        lines.append(s)
        except OSError:
            continue
    return lines[:6]


def report():
    crates, missing = {}, []
    for lock in LOCKS:
        p = os.path.join(ROOT, lock)
        if not os.path.isfile(p):
            continue
        for name, version in locked(p):
            if OURS.match(name):
                continue
            crates.setdefault((name, version), []).append(lock.split("/")[1])
    out = ["# Third-party Rust crates in Tessera's programs", "",
           "tessera-ui and tessera-events are built from these crates (crates.io), at these versions. "
           "Each keeps its own licence; the copyright lines below are the crates' own. "
           "Slint is used under GPL-3.0-only, so the tessera-ui program as distributed is GPL-3.0-only "
           "(Tessera's own code stays GPL-3.0-or-later).", "",
           "| Crate | Version | Licence | In | Copyright |", "|---|---|---|---|---|"]
    for (name, version), where in sorted(crates.items()):
        d = crate_dir(name, version)
        if not d:
            missing.append(f"{name} {version}")
            lic, cr = "(sources not found here)", []
        else:
            lic, cr = licence_of(d) or "(not declared)", copyrights(d)
        out.append(f"| {name} | {version} | {lic} | {', '.join(sorted(set(where)))} | {'<br>'.join(cr).replace('|', '/')} |")
    return "\n".join(out) + "\n", missing


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out")
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args(argv)
    text, missing = report()
    if a.check:
        for m in missing:
            print("no sources for", m, file=sys.stderr)
        return 1 if missing else 0
    if a.out:
        with open(a.out, "w") as f:
            f.write(text)
    else:
        sys.stdout.write(text)
    if missing:
        print(f"rust-licences: {len(missing)} crate(s) without sources here", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
