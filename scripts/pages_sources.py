#!/usr/bin/env python3
"""Resolve immutable sources for the two Pages channels."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


FLATPAK_ARCHIVE = "zeff-boy-flatpak-repo.tar.zst"


def select_stable(releases):
    candidates = [
        release for release in releases
        if not release.get("draft") and not release.get("prerelease")
        and re.fullmatch(r"v\d+\.\d+\.\d+(?:\+[0-9A-Za-z.-]+)?", release.get("tag_name", ""))
        and release.get("published_at")
    ]
    if not candidates:
        raise ValueError("No published stable emulator release")
    release = max(candidates, key=lambda item: item["published_at"])
    assets = {asset["name"] for asset in release.get("assets", [])}
    if not {FLATPAK_ARCHIVE, "SHA256SUMS.txt"}.issubset(assets):
        raise ValueError("Latest stable release lacks required signed Flatpak assets")
    return release["tag_name"]


def checked_sha(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value):
        raise ValueError("GitHub did not return an immutable commit SHA")
    return value


def verify_flatpak(directory):
    directory = Path(directory)
    checksums = (directory / "SHA256SUMS.txt").read_text(encoding="utf-8")
    matches = []
    for line in checksums.splitlines():
        match = re.fullmatch(r"([0-9a-f]{64}) [ *](.+)", line)
        if match and match[2] == FLATPAK_ARCHIVE:
            matches.append(match[1])
    if len(matches) != 1:
        raise ValueError("Missing or ambiguous Flatpak release checksum")
    with (directory / FLATPAK_ARCHIVE).open("rb") as archive:
        actual = hashlib.file_digest(archive, "sha256").hexdigest()
    if actual != matches[0]:
        raise ValueError("Flatpak release checksum mismatch")


def github_json(path, paginate=False):
    command = ["gh", "api", path]
    if paginate:
        command.extend(["--paginate", "--slurp"])
    return json.loads(subprocess.check_output(command, text=True))


def resolve(repository):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Invalid GitHub repository")
    pages = github_json(f"repos/{repository}/releases?per_page=100", paginate=True)
    stable_tag = select_stable([release for page in pages for release in page])
    nightly_sha = checked_sha(github_json(f"repos/{repository}/commits/master")["sha"])
    stable_sha = checked_sha(github_json(f"repos/{repository}/commits/{stable_tag}")["sha"])
    return {"nightly_sha": nightly_sha, "stable_sha": stable_sha, "stable_tag": stable_tag}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository")
    parser.add_argument("--verify-flatpak")
    args = parser.parse_args()
    if args.verify_flatpak:
        verify_flatpak(args.verify_flatpak)
        return
    if not args.repository:
        parser.error("--repository is required")
    sources = resolve(args.repository)
    print(json.dumps(sources, sort_keys=True))
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for key, value in sources.items():
            output.write(f"{key}={value}\n")


if __name__ == "__main__":
    main()
