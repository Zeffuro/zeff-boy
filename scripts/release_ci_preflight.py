"""Require current successful CI/browser evidence before building a release."""

import argparse
import json
import os
import re
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


WORKFLOW = ".github/workflows/ci.yml"
BROWSER_JOB = "WASM browser proof (Edge)"
SOURCE_EVENTS = {"push", "schedule", "workflow_dispatch"}
PENDING = {"queued", "in_progress", "waiting", "requested", "pending"}


class GateError(RuntimeError):
    pass


class GitHubAPI:
    def __init__(self, repository, token, api_url="https://api.github.com"):
        if not token:
            raise GateError("A read-only Actions token is required")
        self.prefix = f"{api_url.rstrip('/')}/repos/{repository}"
        self.token = token

    def get(self, path):
        request = Request(self.prefix + path, headers={
            "Authorization": f"Bearer {self.token}",
            "Accept": "application/vnd.github+json",
            "User-Agent": "zeff-boy-release-preflight",
        })
        try:
            with urlopen(request, timeout=30) as response:
                return json.load(response)
        except HTTPError as error:
            raise GateError(f"GitHub Actions API request failed (HTTP {error.code})") from None
        except (OSError, URLError, ValueError):
            raise GateError("GitHub Actions API request failed") from None


def entries(api, endpoint, key):
    separator = "&" if "?" in endpoint else "?"
    for page in range(1, 11):
        response = api.get(f"{endpoint}{separator}per_page=100&page={page}")
        items = response.get(key) if isinstance(response, dict) else None
        if not isinstance(items, list) or any(not isinstance(item, dict) for item in items):
            raise GateError("Invalid GitHub Actions API response")
        yield from items
        if len(items) < 100:
            return
    raise GateError("GitHub Actions API pagination limit exceeded")


def matches(run, sha):
    return (
        run.get("head_sha") == sha
        and run.get("path") == WORKFLOW
        and run.get("event") in SOURCE_EVENTS
    )


def positive_number(value):
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def newest_run(api, sha):
    runs = [run for run in entries(api, f"/actions/workflows/ci.yml/runs?head_sha={sha}", "workflow_runs")
            if matches(run, sha)]
    if not runs:
        raise GateError("No CI run exists for the exact source commit")
    if any(not positive_number(run.get("id")) for run in runs):
        raise GateError("Invalid CI run identity")
    return max(runs, key=lambda run: run["id"])


def check_once(api, sha):
    run_id = newest_run(api, sha)["id"]
    endpoint = f"/actions/runs/{run_id}"
    run = api.get(endpoint)
    if not isinstance(run, dict) or not matches(run, sha) or run.get("id") != run_id:
        raise GateError("CI run does not match the source commit and workflow")
    attempt = run.get("run_attempt")
    if not positive_number(attempt):
        raise GateError("Invalid current CI attempt")
    if run.get("status") in PENDING:
        return None
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise GateError("Newest source CI run did not complete successfully")
    jobs = list(entries(api, f"{endpoint}/attempts/{attempt}/jobs", "jobs"))
    proof = [job for job in jobs if job.get("name") == BROWSER_JOB]
    if len(proof) != 1:
        raise GateError("Current CI attempt has no unique browser runtime proof job")
    job = proof[0]
    if (job.get("run_id") != run_id or job.get("head_sha") != sha
            or job.get("run_attempt", attempt) != attempt):
        raise GateError("Browser proof belongs to a different source or CI attempt")
    if job.get("status") != "completed" or job.get("conclusion") != "success":
        raise GateError("Browser runtime proof was skipped or did not succeed")
    # A rerun can begin while the attempt's completed jobs are being inspected.
    current = api.get(endpoint)
    if not isinstance(current, dict) or not matches(current, sha) or current.get("id") != run_id:
        raise GateError("CI identity changed during verification")
    if (current.get("run_attempt") != attempt or current.get("status") != "completed"
            or current.get("conclusion") != "success"):
        return None
    newest = newest_run(api, sha)
    if newest["id"] != run_id:
        return None
    newest_attempt = newest.get("run_attempt")
    if not positive_number(newest_attempt):
        raise GateError("Invalid latest CI attempt")
    if newest_attempt > attempt:
        return None
    return run_id, attempt


def require_ci(api, sha, timeout_seconds=3300, clock=time.monotonic, sleep=time.sleep, report=print):
    if not isinstance(sha, str) or not re.fullmatch(r"[0-9a-f]{40}", sha) or set(sha) == {"0"}:
        raise GateError("Release source must be a full nonzero 40-character commit SHA")
    if not 0 <= timeout_seconds <= 3300:
        raise GateError("CI wait must be between 0 and 3300 seconds")
    deadline = clock() + timeout_seconds
    while True:
        verified = check_once(api, sha)
        if verified:
            report(f"Verified exact source {sha}: CI run {verified[0]}, attempt {verified[1]}, browser proof success.")
            return verified
        remaining = deadline - clock()
        if remaining <= 0:
            raise GateError("Timed out waiting for source CI to finish")
        report("Source CI is queued or running; checking again in 30 seconds.")
        sleep(min(30, remaining))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sha", default=os.environ.get("GITHUB_SHA", ""))
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", ""))
    parser.add_argument("--timeout-seconds", type=int, default=3300)
    args = parser.parse_args()
    try:
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo):
            raise GateError("A valid GitHub owner/repository is required")
        api = GitHubAPI(args.repo, os.environ.get("GH_TOKEN"), os.environ.get("GITHUB_API_URL", "https://api.github.com"))
        require_ci(api, args.sha, args.timeout_seconds)
    except GateError as error:
        print(f"Release preflight failed: {error}. Run CI on the exact source commit with the browser proof enabled, then retry the release.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
