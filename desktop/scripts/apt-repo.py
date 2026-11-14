#!/usr/bin/env python3
"""Builds and signs the Wolkenfunk APT repository (https://pkg.wolkenfunk.org/apt).

A plain Debian repository from static files, no reprepro/aptly needed: only dpkg-deb and gpg.

    apt-repo.py REPO_DIR [--add PKG.deb ...] [--keep 3] [--gnupghome DIR] [--key FPR]

Layout written into REPO_DIR:
    pool/main/w/wolkenfunk/wolkenfunk_<ver>_<arch>.deb     packages (the newest --keep per arch)
    dists/stable/main/binary-<arch>/Packages{,.gz}         index
    dists/stable/{Release,InRelease,Release.gpg}           signed metadata
    wolkenfunk.asc                                         public signing key
    wolkenfunk.sources                                     ready-made deb822 source (key inline)
    wolkenfunk_<arch>.deb                                  symlink to the newest package

The signing key lives outside the repo (default GNUPGHOME ~/.config/wolkenfunk/apt-gnupg).
"""

import argparse
import email.utils
import gzip
import hashlib
import os
import shutil
import subprocess
import sys
from pathlib import Path

SUITE = "stable"
COMPONENT = "main"
ARCHITECTURES = ["amd64"]
URL = "https://pkg.wolkenfunk.org/apt"
KEY_UID = "Wolkenfunk Archive Signing Key"
# Same marker as the package's after-install script: the package refreshes a file carrying it.
MARKER = "# Managed by the wolkenfunk package"


def run(*cmd: str, **kw) -> str:
    return subprocess.run(cmd, check=True, capture_output=True, text=True, **kw).stdout


def control(deb: Path) -> dict[str, str]:
    fields = {}
    for f in ("Package", "Version", "Architecture"):
        fields[f] = run("dpkg-deb", "-f", str(deb), f).strip()
    return fields


def newer_first(debs: list[Path]) -> list[Path]:
    """Sorts by Debian version, newest first (dpkg --compare-versions, no python-apt)."""
    import functools

    def cmp(a: Path, b: Path) -> int:
        va, vb = control(a)["Version"], control(b)["Version"]
        if va == vb:
            return 0
        gt = subprocess.run(["dpkg", "--compare-versions", va, "gt", vb]).returncode == 0
        return -1 if gt else 1

    return sorted(debs, key=functools.cmp_to_key(cmp))


def hashes(path: Path) -> dict[str, str]:
    h = {"MD5Sum": hashlib.md5(), "SHA1": hashlib.sha1(), "SHA256": hashlib.sha256()}
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            for v in h.values():
                v.update(chunk)
    return {k: v.hexdigest() for k, v in h.items()}


def add(repo: Path, deb: Path) -> Path:
    c = control(deb)
    name = c["Package"]
    dest = repo / "pool" / COMPONENT / name[0] / name / f"{name}_{c['Version']}_{c['Architecture']}.deb"
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists() and hashes(dest)["SHA256"] != hashes(deb)["SHA256"]:
        sys.exit(f"{dest.name} is already published with different content; bump the version")
    shutil.copy2(deb, dest)
    print(f"+ {dest.relative_to(repo)}")
    return dest


def prune(repo: Path, keep: int) -> None:
    groups: dict[tuple[str, str], list[Path]] = {}
    for deb in (repo / "pool").rglob("*.deb"):
        c = control(deb)
        groups.setdefault((c["Package"], c["Architecture"]), []).append(deb)
    for debs in groups.values():
        for old in newer_first(debs)[keep:]:
            print(f"- {old.relative_to(repo)}")
            old.unlink()


def packages_index(repo: Path, arch: str) -> tuple[str, list[Path]]:
    stanzas, debs = [], []
    for deb in sorted((repo / "pool").rglob("*.deb")):
        c = control(deb)
        if c["Architecture"] not in (arch, "all"):
            continue
        debs.append(deb)
        text = run("dpkg-deb", "-f", str(deb)).rstrip("\n")
        h = hashes(deb)
        text += (
            f"\nFilename: {deb.relative_to(repo)}\nSize: {deb.stat().st_size}"
            f"\nMD5sum: {h['MD5Sum']}\nSHA1: {h['SHA1']}\nSHA256: {h['SHA256']}"
        )
        stanzas.append(text)
    return ("\n\n".join(stanzas) + "\n") if stanzas else "", debs


def write_indexes(repo: Path) -> dict[str, list[Path]]:
    dist = repo / "dists" / SUITE
    newest: dict[str, list[Path]] = {}
    for arch in ARCHITECTURES:
        d = dist / COMPONENT / f"binary-{arch}"
        d.mkdir(parents=True, exist_ok=True)
        text, debs = packages_index(repo, arch)
        (d / "Packages").write_text(text)
        with gzip.GzipFile(d / "Packages.gz", "wb", mtime=0) as f:
            f.write(text.encode())
        newest[arch] = newer_first(debs) if debs else []
    files = sorted(p for p in dist.rglob("Packages*") if p.is_file())
    lines = [
        "Origin: Wolkenfunk",
        "Label: Wolkenfunk",
        f"Suite: {SUITE}",
        f"Codename: {SUITE}",
        f"Date: {email.utils.formatdate(usegmt=True)}",
        f"Architectures: {' '.join(ARCHITECTURES)}",
        f"Components: {COMPONENT}",
        "Description: Wolkenfunk, fast clients for Nextcloud Talk",
    ]
    sums = {p: hashes(p) for p in files}
    for algo in ("MD5Sum", "SHA1", "SHA256"):
        lines.append(f"{algo}:")
        for p in files:
            lines.append(f" {sums[p][algo]} {p.stat().st_size:>10} {p.relative_to(dist)}")
    (dist / "Release").write_text("\n".join(lines) + "\n")
    return newest


def sign(repo: Path, gnupghome: str, key: str | None) -> None:
    dist = repo / "dists" / SUITE
    env = {**os.environ, "GNUPGHOME": gnupghome}
    base = ["gpg", "--batch", "--yes", "--pinentry-mode", "loopback", "--passphrase", ""]
    base += ["--local-user", key or KEY_UID]
    subprocess.run(base + ["--clearsign", "-o", str(dist / "InRelease"), str(dist / "Release")], check=True, env=env)
    subprocess.run(base + ["-abs", "-o", str(dist / "Release.gpg"), str(dist / "Release")], check=True, env=env)
    asc = run("gpg", "--armor", "--export-options", "export-minimal", "--export", key or KEY_UID, env=env)
    (repo / "wolkenfunk.asc").write_text(asc)
    inline = "\n".join(" " + (l if l else ".") for l in asc.rstrip("\n").split("\n"))
    (repo / "wolkenfunk.sources").write_text(
        f"{MARKER} (delete this line to manage the file yourself).\n"
        f"Types: deb\nURIs: {URL}\nSuites: {SUITE}\nComponents: {COMPONENT}\n"
        f"Signed-By:\n{inline}\n"
    )


def link_latest(repo: Path, newest: dict[str, list[Path]]) -> None:
    for arch, debs in newest.items():
        link = repo / f"wolkenfunk_{arch}.deb"
        if link.is_symlink() or link.exists():
            link.unlink()
        if debs:
            link.symlink_to(debs[0].relative_to(repo))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repo", type=Path)
    ap.add_argument("--add", type=Path, nargs="*", default=[])
    ap.add_argument("--keep", type=int, default=3, help="versions per package and architecture")
    ap.add_argument("--gnupghome", default=os.environ.get(
        "WOLKENFUNK_GNUPGHOME", str(Path.home() / ".config/wolkenfunk/apt-gnupg")))
    ap.add_argument("--key", help="signing key fingerprint (default: by uid)")
    a = ap.parse_args()

    a.repo.mkdir(parents=True, exist_ok=True)
    for deb in a.add:
        add(a.repo, deb)
    prune(a.repo, a.keep)
    newest = write_indexes(a.repo)
    sign(a.repo, a.gnupghome, a.key)
    link_latest(a.repo, newest)
    print(f"→ {a.repo} signed")


if __name__ == "__main__":
    main()
