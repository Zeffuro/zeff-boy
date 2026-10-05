import contextlib
import copy
import importlib.util
import io
from pathlib import Path
import re
import unittest
from unittest.mock import patch
from urllib.error import HTTPError


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("release_gate", ROOT / "scripts/release_ci_preflight.py")
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)
SHA = "a" * 40
RUNS = f"/actions/workflows/ci.yml/runs?head_sha={SHA}&per_page=100&page=1"


def run(run_id=20, attempt=1, status="completed", conclusion="success", **fields):
    result = {
        "id": run_id, "run_attempt": attempt, "head_sha": SHA, "path": gate.WORKFLOW,
        "event": "push", "status": status, "conclusion": conclusion,
    }
    result.update(fields)
    return result


def proof(run_id=20, attempt=1, **fields):
    result = {
        "name": gate.BROWSER_JOB, "run_id": run_id, "run_attempt": attempt,
        "head_sha": SHA, "status": "completed", "conclusion": "success",
    }
    result.update(fields)
    return result


class FakeAPI:
    def __init__(self, current=None, jobs=None, runs=None):
        current = current or run()
        self.calls = []
        self.responses = {
            RUNS: {"workflow_runs": runs if runs is not None else [current]},
            f"/actions/runs/{current['id']}": current,
            f"/actions/runs/{current['id']}/attempts/{current['run_attempt']}/jobs?per_page=100&page=1": {
                "jobs": jobs if jobs is not None else [proof(current["id"], current["run_attempt"])]
            },
        }

    def get(self, path):
        self.calls.append(path)
        response = self.responses[path]
        return copy.deepcopy(response() if callable(response) else response)


class Clock:
    def __init__(self):
        self.now = 0
        self.sleeps = []

    def __call__(self):
        return self.now

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.now += seconds


class PreflightTests(unittest.TestCase):
    def require(self, api, timeout=0, clock=None):
        clock = clock or Clock()
        return gate.require_ci(api, SHA, timeout, clock=clock, sleep=clock.sleep, report=lambda _: None)

    def test_success_requires_current_attempt_job_and_overall_ci(self):
        api = FakeAPI(current=run(attempt=3), jobs=[proof(attempt=3)])
        self.assertEqual(self.require(api), (20, 3))
        self.assertIn("/actions/runs/20/attempts/3/jobs?per_page=100&page=1", api.calls)
        self.assertEqual(api.calls.count("/actions/runs/20"), 2)

    def test_invalid_sha_and_wait_limit_fail_before_api(self):
        api = FakeAPI()
        for sha in ["a" * 7, "g" * 40, "0" * 40, SHA + "\n", None]:
            with self.subTest(sha=sha), self.assertRaises(gate.GateError):
                gate.require_ci(api, sha, report=lambda _: None)
        for timeout in [-1, 3301]:
            with self.assertRaises(gate.GateError):
                gate.require_ci(api, SHA, timeout, report=lambda _: None)
        self.assertEqual(api.calls, [])

    def test_missing_wrong_sha_workflow_and_pr_runs_do_not_qualify(self):
        for runs in [[], [run(head_sha="b" * 40)], [run(path=".github/workflows/lobby.yml")], [run(event="pull_request")]]:
            with self.subTest(runs=runs), self.assertRaisesRegex(gate.GateError, "No CI run"):
                self.require(FakeAPI(runs=runs))

    def test_run_detail_cannot_change_sha_or_workflow(self):
        for changed in [run(head_sha="b" * 40), run(path=".github/workflows/lobby.yml"), run(run_id=21)]:
            api = FakeAPI()
            api.responses["/actions/runs/20"] = changed
            with self.subTest(changed=changed), self.assertRaisesRegex(gate.GateError, "does not match"):
                self.require(api)

    def test_newest_failure_cannot_use_older_success(self):
        api = FakeAPI(current=run(run_id=21, conclusion="failure"), runs=[run(), run(run_id=21)])
        with self.assertRaisesRegex(gate.GateError, "Newest source CI"):
            self.require(api)
        self.assertNotIn("/actions/runs/20", api.calls)

    def test_newest_pending_cannot_use_older_success(self):
        api = FakeAPI(current=run(run_id=21, status="queued", conclusion=None), runs=[run(), run(run_id=21)])
        with self.assertRaisesRegex(gate.GateError, "Timed out"):
            self.require(api)
        self.assertNotIn("/actions/runs/20", api.calls)

    def test_failed_cancelled_and_unknown_runs_fail_even_with_successful_job(self):
        for status, conclusion in [("completed", "failure"), ("completed", "cancelled"), ("completed", "neutral"), ("unknown", "success")]:
            with self.subTest(status=status, conclusion=conclusion), self.assertRaises(gate.GateError):
                self.require(FakeAPI(current=run(status=status, conclusion=conclusion)))

    def test_missing_duplicate_skipped_failed_and_unfinished_browser_jobs_fail(self):
        for jobs in [
            [], [proof(name="WASM build")], [proof(), proof()],
            [proof(conclusion="skipped")], [proof(conclusion="failure")],
            [proof(status="in_progress", conclusion=None)],
        ]:
            with self.subTest(jobs=jobs), self.assertRaises(gate.GateError):
                self.require(FakeAPI(jobs=jobs))

    def test_old_attempt_or_wrong_source_jobs_fail(self):
        for job in [proof(attempt=1), proof(attempt=2, head_sha="b" * 40), proof(run_id=19, attempt=2)]:
            with self.subTest(job=job), self.assertRaisesRegex(gate.GateError, "different source or CI attempt"):
                self.require(FakeAPI(current=run(attempt=2), jobs=[job]))

    def test_stale_list_attempt_does_not_select_old_attempt_jobs(self):
        api = FakeAPI(current=run(attempt=2), runs=[run(attempt=1)])
        self.assertEqual(self.require(api), (20, 2))
        self.assertNotIn("/actions/runs/20/attempts/1/jobs?per_page=100&page=1", api.calls)

    def test_rerun_start_during_job_inspection_rechecks_current_attempt(self):
        api = FakeAPI()
        current = iter([run(), run(attempt=2, status="queued", conclusion=None)])
        api.responses["/actions/runs/20"] = lambda: next(current)
        with self.assertRaisesRegex(gate.GateError, "Timed out"):
            self.require(api)

    def test_final_list_fences_newer_same_run_attempt(self):
        api = FakeAPI()
        snapshots = iter([
            {"workflow_runs": [run()]},
            {"workflow_runs": [run(attempt=2, status="queued", conclusion=None)]},
        ])
        api.responses[RUNS] = lambda: next(snapshots)
        with self.assertRaisesRegex(gate.GateError, "Timed out"):
            self.require(api)
        self.assertEqual(api.calls.count("/actions/runs/20"), 2)
        self.assertEqual(api.calls.count(RUNS), 2)

    def test_final_list_rejects_invalid_same_run_attempt(self):
        for attempt in [None, 0, True, "2"]:
            api = FakeAPI()
            snapshots = iter([
                {"workflow_runs": [run()]}, {"workflow_runs": [run(attempt=attempt)]},
            ])
            api.responses[RUNS] = lambda: next(snapshots)
            with self.subTest(attempt=attempt), self.assertRaisesRegex(gate.GateError, "Invalid latest CI attempt"):
                self.require(api)

    def test_newer_run_during_proof_inspection_cannot_use_older_success(self):
        for status, conclusion, expected in [
            ("queued", None, "Timed out"), ("completed", "failure", "Newest source CI"),
        ]:
            with self.subTest(status=status):
                api = FakeAPI()
                inspected = {"proof": False}
                newer = run(run_id=21, status=status, conclusion=conclusion, event="workflow_dispatch")
                api.responses[RUNS] = lambda: {
                    "workflow_runs": [run(), newer] if inspected["proof"] else [run()]
                }
                api.responses["/actions/runs/21"] = newer

                def inspect_proof():
                    inspected["proof"] = True
                    return {"jobs": [proof()]}

                api.responses["/actions/runs/20/attempts/1/jobs?per_page=100&page=1"] = inspect_proof
                with self.assertRaisesRegex(gate.GateError, expected):
                    self.require(api, timeout=60)
                self.assertIn("/actions/runs/21", api.calls)
                self.assertGreaterEqual(api.calls.count(RUNS), 3)

    def test_queued_running_then_success_polls_every_30_seconds(self):
        api = FakeAPI()
        current = iter([run(status="queued", conclusion=None), run(status="in_progress", conclusion=None), run(), run()])
        api.responses["/actions/runs/20"] = lambda: next(current)
        clock = Clock()
        self.assertEqual(self.require(api, timeout=100, clock=clock), (20, 1))
        self.assertEqual(clock.sleeps, [30, 30])

    def test_running_then_failure_fails_without_waiting_full_timeout(self):
        api = FakeAPI()
        current = iter([run(status="in_progress", conclusion=None), run(conclusion="failure")])
        api.responses["/actions/runs/20"] = lambda: next(current)
        clock = Clock()
        with self.assertRaisesRegex(gate.GateError, "Newest source CI"):
            self.require(api, timeout=100, clock=clock)
        self.assertEqual(clock.sleeps, [30])

    def test_polling_is_bounded(self):
        api = FakeAPI(current=run(status="queued", conclusion=None))
        clock = Clock()
        with self.assertRaisesRegex(gate.GateError, "Timed out"):
            self.require(api, timeout=35, clock=clock)
        self.assertEqual(clock.sleeps, [30, 5])

    def test_jobs_are_paginated(self):
        api = FakeAPI()
        api.responses["/actions/runs/20/attempts/1/jobs?per_page=100&page=1"] = {
            "jobs": [proof(name=f"other-{index}") for index in range(100)]
        }
        api.responses["/actions/runs/20/attempts/1/jobs?per_page=100&page=2"] = {"jobs": [proof()]}
        self.assertEqual(self.require(api), (20, 1))

    def test_malformed_api_responses_fail_closed(self):
        for response in [[], {}, {"workflow_runs": [None]}]:
            api = FakeAPI()
            api.responses[RUNS] = response
            with self.subTest(response=response), self.assertRaises(gate.GateError):
                self.require(api)

    def test_api_errors_never_log_token_or_response_body(self):
        api = gate.GitHubAPI("owner/repo", "private-token")
        error = HTTPError("https://api.github.com", 403, "private-token", {}, None)
        with patch.object(gate, "urlopen", side_effect=error):
            with self.assertRaises(gate.GateError) as caught:
                api.get("/actions/runs/20")
        self.assertIn("HTTP 403", str(caught.exception))
        self.assertNotIn("private-token", str(caught.exception))

    def test_failure_instructs_running_source_ci(self):
        output = io.StringIO()
        with patch("sys.argv", ["release_ci_preflight.py", "--sha", SHA, "--repo", "owner/repo"]), \
                patch.object(gate, "GitHubAPI", return_value=FakeAPI(runs=[])), \
                contextlib.redirect_stderr(output):
            self.assertEqual(gate.main(), 1)
        self.assertIn("Run CI on the exact source commit with the browser proof enabled", output.getvalue())


class ReleaseWorkflowTests(unittest.TestCase):
    def test_all_release_roots_depend_on_read_only_preflight(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        sections = re.split(r"^  ([a-z][a-z-]*):\n", workflow.split("jobs:\n", 1)[1], flags=re.MULTILINE)
        jobs = dict(zip(sections[1::2], sections[2::2]))
        for name in ["build", "libretro", "flatpak"]:
            self.assertIn("    needs: preflight\n", jobs[name])
        for name, body in jobs.items():
            if name != "preflight":
                self.assertIn("    needs:", body, name)
        preflight = jobs["preflight"]
        self.assertIn("    timeout-minutes: 60", preflight)
        self.assertIn("      actions: read", preflight)
        self.assertIn("      contents: read", preflight)
        self.assertNotIn(": write", preflight)
        self.assertIn("python scripts/release_ci_preflight.py --timeout-seconds 3300", preflight)
        self.assertIn('- "v*"', workflow)
        self.assertNotIn("lobby-v", workflow)


if __name__ == "__main__":
    unittest.main()
