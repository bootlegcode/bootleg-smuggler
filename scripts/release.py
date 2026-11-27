#!/usr/bin/env python3
"""Prepare a version bump and push main and its release tag together."""

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib


def run(*args, env=None):
    result = subprocess.run(
        args, check=True, text=True, stdout=subprocess.PIPE, env=env
    )
    return result.stdout.strip()


def release(part):
    if run("git", "branch", "--show-current") != "main":
        raise ValueError("Run releases from main.")
    if run("git", "status", "--porcelain", "--untracked-files=normal"):
        raise ValueError("Commit or stash local changes before releasing.")

    manifest = Path("Cargo.toml")
    source = manifest.read_text(encoding="utf-8")
    current = tomllib.loads(source)["package"]["version"]
    # Always use the local Git backend, regardless of Vinnie's user configuration.
    vinnie_env = {
        key: value for key, value in os.environ.items() if not key.startswith("VINNIE_")
    }
    vinnie = (
        "vinnie",
        "--repo",
        ".",
        "--semver",
        "--no-push",
        "--prefix",
        "v",
        "--current-version",
        current,
    )
    version = run(*vinnie, "--omit-prefix", "next", part, env=vinnie_env)
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) or version == current:
        raise ValueError(f"Unexpected next version from vinnie: {version!r}")
    tag = f"v{version}"
    tag_ref = f"refs/tags/{tag}"

    run("git", "fetch", "--no-tags", "origin", "refs/heads/main")
    if run("git", "rev-parse", "HEAD") != run("git", "rev-parse", "FETCH_HEAD"):
        raise ValueError("Local main must match origin/main before releasing.")
    if run("git", "tag", "--list", tag) or run(
        "git", "ls-remote", "--refs", "origin", tag_ref
    ):
        raise ValueError(f"Tag {tag} already exists locally or on origin.")

    # Change only [package].version, preserving the manifest's formatting.
    package = re.search(r"(?m)^\[package\][^\n]*\n(?:^(?!\s*\[)[^\n]*\n)*", source)
    version_line = rf'(?m)^(version\s*=\s*)"{re.escape(current)}"'
    updated, count = re.subn(
        version_line, rf'\g<1>"{version}"', package[0] if package else ""
    )
    if count != 1:
        raise ValueError(
            "Could not locate the version line in Cargo.toml's [package] section."
        )
    manifest.write_text(
        source[: package.start()] + updated + source[package.end() :], encoding="utf-8"
    )
    subprocess.run(["cargo", "update", "--workspace"], check=True)
    subprocess.run(
        ["git", "commit", "-m", f"Release {tag}", "--", "Cargo.toml", "Cargo.lock"],
        check=True,
    )
    run(*vinnie, part, env=vinnie_env)
    if run("git", "rev-parse", f"{tag_ref}^{{commit}}") != run(
        "git", "rev-parse", "HEAD"
    ):
        raise ValueError(f"Vinnie did not tag the release commit as {tag}.")

    push = [
        "git",
        "push",
        "--atomic",
        "--no-follow-tags",
        "origin",
        "HEAD:refs/heads/main",
        f"{tag_ref}:{tag_ref}",
    ]
    try:
        subprocess.run(push, check=True)
    except subprocess.CalledProcessError:
        print(
            f"Release commit and {tag} are local. After fixing the push failure, retry:\n"
            f"  {' '.join(push)}",
            file=sys.stderr,
        )
        raise
    print(f"Pushed {tag}; GitHub Actions will run the release workflow.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("part", choices=("patch", "minor", "major"))
    args = parser.parse_args()
    try:
        release(args.part)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"Release failed: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stdout:
            print(error.stdout.strip(), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
