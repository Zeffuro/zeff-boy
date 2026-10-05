"""Select the full browser proof or a netplay smoke from a GitHub event."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess


SHA = re.compile(r"[0-9a-fA-F]{40}(?:[0-9a-fA-F]{24})?\Z")
FULL_EVENTS = {"schedule", "workflow_dispatch"}
ROOT_INPUTS = {
    "Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain", "rust-toolchain.toml",
    "Trunk.toml", "index.html", ".github/workflows/ci.yml",
    "scripts/test-wasm-browser-speculation.ps1", "scripts/browser_netplay_gate.py",
    "scripts/tests/test_browser_netplay_gate.py", "scripts/web_bundle.py",
}
CRATE_PREFIXES = (
    "crates/zeff-netplay/", "crates/zeff-netplay-connect/",
    "crates/zeff-netplay-protocol/", "crates/zeff-netplay-lobby/",
    "crates/zeff-emu-common/", "crates/zeff-firmware/",
    "crates/zeff-test-support/", "crates/zeff-z80/",
    "crates/zeff-nes-core/", "crates/zeff-sega8-core/", "crates/zeff-pce-core/",
    "crates/zeff-ws-core/", "crates/zeff-gb-core/", "crates/zeff-gba-core/",
    "crates/zeff-coleco-core/",
)
RUNTIME_SUFFIXES = {
    ".rs", ".toml", ".js", ".ts", ".wgsl", ".json", ".html",
    ".bin", ".dat", ".nes", ".sms", ".sg", ".ws", ".wsc", ".pce", ".zip",
}


def relevant(path):
    if path in ROOT_INPUTS or Path(path).name in {"Cargo.toml", "Cargo.lock", "build.rs"}:
        return True
    if path.startswith((".cargo/", "src/", "tests/", *CRATE_PREFIXES)):
        return Path(path).suffix in RUNTIME_SUFFIXES
    return False


def valid_sha(value):
    return isinstance(value, str) and SHA.fullmatch(value) and set(value) != {"0"}


def git(repo, *args):
    result = subprocess.run(
        ["git", "-C", str(repo), *args], check=True, capture_output=True, timeout=60
    )
    return result.stdout


def changed_paths(event_name, event, repo):
    if event_name == "push":
        base, head = event.get("before"), event.get("after")
    elif event_name == "pull_request":
        request = event["pull_request"]
        base, head = request["base"]["sha"], request["head"]["sha"]
    else:
        raise ValueError("Unrecognized event")
    if not valid_sha(base) or not valid_sha(head):
        raise ValueError("Missing or invalid comparison SHA")
    if event_name == "pull_request":
        base = git(repo, "merge-base", base, head).decode("ascii").strip()
        if not valid_sha(base):
            raise ValueError("Unavailable pull request merge base")
    # Disabling rename detection includes both old and new paths in the gate.
    names = git(repo, "diff", "--name-only", "--no-renames", "-z", base, head, "--")
    return names.decode("utf-8", errors="surrogateescape").split("\0")[:-1]


def select_suite(event_name, event_path, repo):
    if event_name in FULL_EVENTS:
        return "full", "Scheduled or manual browser proof"
    try:
        event = json.loads(Path(event_path).read_text(encoding="utf-8"))
        if not isinstance(event, dict):
            raise ValueError("Invalid event object")
        paths = changed_paths(event_name, event, repo)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        return "smoke", "Comparison unavailable; running netplay smoke"
    if any(relevant(path) for path in paths):
        return "smoke", "Browser netplay runtime inputs changed"
    return "skip", "No browser netplay runtime inputs changed"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event-name", default=os.environ.get("GITHUB_EVENT_NAME", ""))
    parser.add_argument("--event-path", default=os.environ.get("GITHUB_EVENT_PATH", ""))
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--output", default=os.environ.get("GITHUB_OUTPUT"))
    args = parser.parse_args()
    suite, reason = select_suite(args.event_name, args.event_path, args.repo)
    print(f"Browser suite: {suite}. {reason}.")
    if args.output:
        with open(args.output, "a", encoding="utf-8") as output:
            output.write(f"suite={suite}\n")


if __name__ == "__main__":
    main()
