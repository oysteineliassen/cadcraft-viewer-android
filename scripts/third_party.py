#!/usr/bin/env python3
"""Skriver licenses/THIRD-PARTY.md: alle Rust-biblioteker som bygges inn i Android-appen,
med versjon, lisens og kilde. Kjør fra repo-roten etter endringer i Cargo.lock:

    python3 scripts/third_party.py
"""
import json
import subprocess

TARGET = "aarch64-linux-android"
meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", TARGET]))
pkgs = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
root = meta["resolve"]["root"]

seen, stack = set(), [root]
while stack:
    pid = stack.pop()
    if pid in seen:
        continue
    seen.add(pid)
    for dep in nodes[pid]["deps"]:
        # Bare vanlige avhengigheter (ikke build- eller dev-) havner i appen, men build-
        # avhengigheter tas med for fullstendighet når de også er vanlige.
        if any(k["kind"] is None for k in dep["dep_kinds"]):
            stack.append(dep["pkg"])
seen.discard(root)

rows = sorted((pkgs[p]["name"], pkgs[p]["version"], pkgs[p].get("license") or "se kilde",
               pkgs[p].get("repository") or "") for p in seen)
with open("licenses/THIRD-PARTY.md", "w", encoding="utf-8") as f:
    f.write("Rust-biblioteker i CAD-viser for Android (generert av scripts/third_party.py).\n")
    f.write("Lisenstekstene følger med kildekoden til hvert bibliotek på crates.io eller i repoet.\n\n")
    for name, ver, lic, repo in rows:
        f.write(f"{name} {ver} - {lic}" + (f" - {repo}" if repo else "") + "\n")
print(f"{len(rows)} biblioteker")
