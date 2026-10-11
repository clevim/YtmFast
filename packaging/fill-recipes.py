#!/usr/bin/env python3
"""Fills the AUR recipe templates with a release's version and checksums.

    python3 packaging/fill-recipes.py DIST VERSION OUT

DIST holds the release files and their checksums.txt (sha256, `sum  name` per line); OUT gets
arch/ytmfast/PKGBUILD and arch/ytmfast-bin/PKGBUILD.
"""
import pathlib
import sys

dist, version, out = pathlib.Path(sys.argv[1]), sys.argv[2], pathlib.Path(sys.argv[3])
sums = {}
for line in (dist / "checksums.txt").read_text().splitlines():
    digest, name = line.split(maxsplit=1)
    sums[name.strip().lstrip("*")] = digest

def sha(name):
    if name not in sums:
        sys.exit(f"missing from checksums.txt: {name}")
    return sums[name]

v = f"v{version}"
values = {
    "@VERSION@": version,
    "@SHA256_SOURCE@": sha(f"ytmfast-{v}-source.tar.gz"),
    "@SHA256_LINUX_X86_64@": sha(f"ytmfast-{v}-x86_64-unknown-linux-gnu.tar.gz"),
    "@SHA256_LINUX_AARCH64@": sha(f"ytmfast-{v}-aarch64-unknown-linux-gnu.tar.gz"),
}
root = pathlib.Path(__file__).parent
for template, target in [
    ("arch/ytmfast/PKGBUILD.in", "arch/ytmfast/PKGBUILD"),
    ("arch/ytmfast-bin/PKGBUILD.in", "arch/ytmfast-bin/PKGBUILD"),
]:
    text = (root / template).read_text()
    for k, val in values.items():
        text = text.replace(k, val)
    dest = out / target
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(text)
    print(dest)
