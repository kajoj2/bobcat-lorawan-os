#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""List recipes whose upstream project has a newer release than the recipe's version.

    tools/check-upstream.py [LAYER ...]        (default: every meta-* directory)

Looks at recipes named <name>_<version>.bb that fetch a GitHub repository by tag (`tag=v${PV}`, `tag=V${PV}`, ...),
and compares the version with the repository's latest release, or with its newest tag when it has no releases.
Prints a Markdown report of the outdated recipes (nothing when all are current). Set GITHUB_TOKEN to avoid the API's
anonymous rate limit.
"""
import json
import os
import pathlib
import re
import sys
import urllib.error
import urllib.request

SRC_RE = re.compile(r"git://github\.com/([\w.-]+)/([\w.-]+?)(?:\.git)?;[^\s\"]*\btag=([^;\s\"]*)\$\{PV\}")


def api(path):
    req = urllib.request.Request(f"https://api.github.com/{path}", headers={"Accept": "application/vnd.github+json"})
    if token := os.environ.get("GITHUB_TOKEN"):
        req.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(req) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None
        raise


def vkey(version):
    """2.1.0r9 -> (2, 1, 0, 9); good enough for the version schemes of our upstreams."""
    return tuple(int(n) for n in re.findall(r"\d+", version))


def latest(owner, repo, prefix):
    rel = api(f"repos/{owner}/{repo}/releases/latest")
    if rel and rel["tag_name"].startswith(prefix):
        return rel["tag_name"][len(prefix):], rel["html_url"]
    tags = [t["name"][len(prefix):] for t in api(f"repos/{owner}/{repo}/tags?per_page=100") or []
            if t["name"].startswith(prefix) and re.fullmatch(r"[\d.]+(?:r\d+)?", t["name"][len(prefix):])]
    if not tags:
        return None, None
    best = max(tags, key=vkey)
    return best, f"https://github.com/{owner}/{repo}/releases/tag/{prefix}{best}"


def main():
    layers = [pathlib.Path(p) for p in sys.argv[1:]] or sorted(pathlib.Path(".").glob("meta-*"))
    rows = []
    for bb in sorted(p for layer in layers for p in layer.rglob("*_*.bb")):
        name, _, pv = bb.stem.rpartition("_")
        m = SRC_RE.search(bb.read_text())
        if not m:
            continue
        owner, repo, prefix = m.groups()
        new, url = latest(owner, repo, prefix)
        if new and vkey(new) > vkey(pv):
            rows.append(f"| `{name}` | {pv} | [{new}]({url}) | `{bb.as_posix()}` |")
    if not rows:
        return
    print("Upstream has newer releases for these recipes:\n")
    print("| Recipe | Recipe version | Latest upstream | File |")
    print("|---|---|---|---|")
    print("\n".join(rows))
    print("\nHow to update one, including its crate list for Rust recipes: docs/building.md, "
          "\"Updating a Rust component\". This issue is rewritten by the `updates` workflow every week and closed "
          "when nothing is outdated.")


if __name__ == "__main__":
    main()
