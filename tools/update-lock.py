#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Move the layer revisions in a kas lock file to the heads of their branches.

    tools/update-lock.py kas/bobcat-g285.yml [--hold REPO ...]

Reads the repositories (url, branch, defaults.repos.branch) from the kas file, asks each remote for its branch head
with `git ls-remote`, and rewrites the `commit:` lines of the lock file next to it (<name>.lock.yml) in place.
Repositories named with --hold keep their revision (meta-rockchip: pinned on purpose, see the kas file).

Prints a Markdown summary of what moved, with the new commits of each repository (from a shallow fetch) and a link
to the full range, for the body of a pull request. Prints nothing when nothing moved.
"""
import argparse
import pathlib
import re
import subprocess
import sys
import tempfile

import yaml

MAX_LOG = 60  # commits listed per repository; the link has the rest
FETCH_DEPTH = 400


def git(*args, cwd=None):
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout


def branch_head(url, branch):
    out = git("ls-remote", url, f"refs/heads/{branch}")
    if not out:
        sys.exit(f"{url}: no branch {branch}")
    return out.split()[0]


def range_link(url, branch, old, new):
    if url.startswith("https://github.com/"):
        return f"{url.removesuffix('.git')}/compare/{old}...{new}"
    # git.openembedded.org and git.yoctoproject.org run cgit
    return f"{url}/log/?h={branch}&qt=range&q={old}..{new}"


def new_commits(url, branch, old, new):
    """One line per commit in old..new, or None when old is deeper than the shallow fetch."""
    with tempfile.TemporaryDirectory() as d:
        git("init", "-q", "--bare", d)
        git("fetch", "-q", f"--depth={FETCH_DEPTH}", url, f"refs/heads/{branch}", cwd=d)
        try:
            git("cat-file", "-e", f"{old}^{{commit}}", cwd=d)
        except subprocess.CalledProcessError:
            return None
        return git("log", "--format=%h %s", f"{old}..{new}", cwd=d).splitlines()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("kasfile", type=pathlib.Path)
    ap.add_argument("--hold", action="append", default=[], metavar="REPO")
    args = ap.parse_args()

    kas = yaml.safe_load(args.kasfile.read_text())
    lockfile = args.kasfile.with_name(args.kasfile.stem + ".lock.yml")
    lock_text = lockfile.read_text()
    locked = yaml.safe_load(lock_text)["overrides"]["repos"]
    default_branch = (kas.get("defaults") or {}).get("repos", {}).get("branch")

    sections = []
    for name, repo in (kas.get("repos") or {}).items():
        repo = repo or {}
        url = repo.get("url")
        if not url or name not in locked:
            continue  # this repository itself, or not locked
        if name in args.hold:
            continue
        branch = repo.get("branch", default_branch)
        old = locked[name]["commit"]
        new = branch_head(url, branch)
        if new == old:
            continue

        pattern = re.compile(rf"(^    {re.escape(name)}:\n      commit: ){old}$", re.M)
        lock_text, n = pattern.subn(rf"\g<1>{new}", lock_text)
        if n != 1:
            sys.exit(f"{lockfile}: could not find the commit line of {name}")

        log = new_commits(url, branch, old, new)
        lines = [f"### {name} ({branch}): `{old[:12]}` → `{new[:12]}`", ""]
        if log is None:
            lines.append(f"More than {FETCH_DEPTH} commits.")
        else:
            lines.append(f"{len(log)} commit{'s' if len(log) != 1 else ''}:")
            lines.append("")
            lines += [f"- {line}" for line in log[:MAX_LOG]]
            if len(log) > MAX_LOG:
                lines.append(f"- … and {len(log) - MAX_LOG} more")
        lines += ["", f"Full range: {range_link(url, branch, old, new)}", ""]
        sections.append("\n".join(lines))

    if not sections:
        return
    lockfile.write_text(lock_text)
    held = ", ".join(f"`{h}`" for h in args.hold) or "none"
    print(f"Layer revisions in `{lockfile.as_posix()}` moved to their branch heads (held back: {held}).\n")
    print("\n".join(sections))
    print("Before merging: build the image and the update bundle on a build host and test them on a board "
          "(docs/building.md). CI here parses the layers and builds the toolchain, kernel and U-Boot only.")


if __name__ == "__main__":
    main()
