import importlib.util
import json
from pathlib import Path
import re
import subprocess
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("browser_gate", ROOT / "scripts/browser_netplay_gate.py")
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class EventGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.run_git("init", "--initial-branch=main")
        self.run_git("config", "user.name", "Fixture")
        self.run_git("config", "user.email", "fixture@example.invalid")
        self.base = self.commit("README.md", "base")

    def run_git(self, *args):
        return subprocess.run(
            ["git", "-C", str(self.repo), *args], check=True, capture_output=True,
            text=True, encoding="utf-8",
        ).stdout.strip()

    def commit(self, path, content):
        destination = self.repo / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(content, encoding="utf-8")
        self.run_git("add", "--all")
        self.run_git("commit", "-m", "Fixture")
        return self.run_git("rev-parse", "HEAD")

    def select(self, name, event):
        event_path = self.repo / "event.json"
        event_path.write_text(json.dumps(event), encoding="utf-8")
        return gate.select_suite(name, event_path, self.repo)[0]

    def push(self, before, after):
        return self.select("push", {"before": before, "after": after})

    def test_docs_only_push_skips(self):
        head = self.commit("docs/netplay.md", "docs")
        self.assertEqual(self.push(self.base, head), "skip")

    def test_batch_push_includes_earlier_runtime_change(self):
        self.commit("src/netplay/session.rs", "runtime")
        head = self.commit("README.md", "docs")
        self.assertEqual(self.push(self.base, head), "smoke")

    def test_pull_request_uses_merge_base_not_current_base_tree(self):
        self.run_git("checkout", "-b", "feature")
        head = self.commit("docs/browser.md", "docs")
        self.run_git("checkout", "main")
        base = self.commit("src/netplay/session.rs", "base-only runtime")
        event = {"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}
        self.assertEqual(self.select("pull_request", event), "skip")
        self.run_git("checkout", "feature")
        head = self.commit("src/emu_thread/wasm.rs", "runtime")
        event["pull_request"]["head"]["sha"] = head
        self.assertEqual(self.select("pull_request", event), "smoke")

    def test_deleted_runtime_file_runs(self):
        base = self.commit("src/netplay/session.rs", "runtime")
        self.run_git("rm", "src/netplay/session.rs")
        self.run_git("commit", "-m", "Delete fixture")
        self.assertEqual(self.push(base, self.run_git("rev-parse", "HEAD")), "smoke")

    def test_rename_outside_runtime_scope_includes_old_path(self):
        base = self.commit("src/netplay/session.rs", "runtime")
        self.run_git("mv", "src/netplay/session.rs", "archived.rs")
        self.run_git("commit", "-m", "Move fixture")
        self.assertEqual(self.push(base, self.run_git("rev-parse", "HEAD")), "smoke")

    def test_first_push_deleted_branch_and_unavailable_objects_fail_closed(self):
        for before, after in [
            ("0" * 40, self.base), (self.base, "0" * 40), ("f" * 40, self.base),
            (self.base, "--output=untrusted"), (None, self.base),
        ]:
            with self.subTest(before=before, after=after):
                self.assertEqual(self.push(before, after), "smoke")

    def test_missing_merge_base_fails_closed(self):
        event = {"pull_request": {"base": {"sha": "f" * 40}, "head": {"sha": self.base}}}
        self.assertEqual(self.select("pull_request", event), "smoke")

    def test_corrupt_missing_and_unknown_events_fail_closed(self):
        event = self.repo / "corrupt.json"
        event.write_text("{broken", encoding="utf-8")
        self.assertEqual(gate.select_suite("push", event, self.repo)[0], "smoke")
        self.assertEqual(gate.select_suite("push", event.with_name("missing"), self.repo)[0], "smoke")
        self.assertEqual(self.select("push", {}), "smoke")
        self.assertEqual(self.select("push", []), "smoke")
        self.assertEqual(self.select("unknown", {}), "smoke")

    def test_git_failure_and_timeout_fail_closed(self):
        for failure in [subprocess.CalledProcessError(128, "git"), subprocess.TimeoutExpired("git", 60)]:
            with self.subTest(failure=failure), patch.object(gate, "git", side_effect=failure):
                self.assertEqual(self.push(self.base, self.base), "smoke")

    def test_schedule_and_manual_keep_full_suite_without_event_file(self):
        for name in ["schedule", "workflow_dispatch"]:
            self.assertEqual(gate.select_suite(name, "missing", self.repo)[0], "full")

    def test_empty_comparison_skips(self):
        self.assertEqual(self.push(self.base, self.base), "skip")

    def test_path_coverage_and_docs_exclusions(self):
        for path in [
            "src/app.rs", "src/app/netplay.rs", "src/app/state_io/wasm_rom.rs",
            "src/emu_thread/wasm/netplay.rs", "src/emu_backend/nes.rs",
            "src/audio/web.rs", "src/platform/storage.rs", "src/input/mod.rs",
            "crates/zeff-netplay-protocol/src/lib.rs", "crates/zeff-netplay-connect/src/browser.rs",
            "crates/zeff-netplay/src/session.rs", "crates/zeff-netplay-lobby/src/main.rs",
            "crates/zeff-ws-core/src/save_state.rs", "crates/zeff-nes-core/src/apu.rs",
            "crates/zeff-pce-core/src/lib.rs", "crates/zeff-sega8-core/Cargo.toml",
            "crates/zeff-emu-common/src/lib.rs", "Cargo.lock", "Cargo.toml",
            ".cargo/config.toml", ".github/workflows/ci.yml",
            "scripts/test-wasm-browser-speculation.ps1", "scripts/browser_netplay_gate.py",
            "scripts/tests/test_browser_netplay_gate.py",
            "crates/zeff-ws-core/tests/fixture.bin", "crates/other/Cargo.toml",
        ]:
            with self.subTest(path=path):
                self.assertTrue(gate.relevant(path))
        for path in ["README.md", "docs/netplay.md", "crates/zeff-netplay/README.md", "src/app/notes.md", ".cargo/README.md"]:
            with self.subTest(path=path):
                self.assertFalse(gate.relevant(path))


class BrowserRoutingTests(unittest.TestCase):
    def test_smoke_filters_select_existing_gameplay_fixtures(self):
        script = (ROOT / "scripts/test-wasm-browser-speculation.ps1").read_text()
        smoke = script.split("if ($NetplaySmoke) {", 1)[1].split("} elseif ($Netplay) {", 1)[0]
        filters = re.findall(r'"(browser_netplay_[^"]+)"', smoke)
        self.assertEqual(len(filters), 3)
        sources = "\n".join(path.read_text() for path in (ROOT / "src/emu_thread/wasm/netplay").rglob("*.rs"))
        for name in filters:
            self.assertEqual(len(re.findall(rf"async fn {re.escape(name)}\(", sources)), 1)
        self.assertIn("browser_netplay_actual_worker_matches_reference_and_restores_sram", filters)
        self.assertIn("browser_netplay_ws_worker_replicates_link_and_restores", filters)
        self.assertIn("browser_netplay_build_mismatch_and_suspended_peer_restore_before_saving", filters)
        self.assertIn("$runNetplay = $Netplay -or $NetplaySmoke", script)
        self.assertIn("--features browser-tests browser_", script)
        self.assertIn("if ($runNetplay) {", script)
        self.assertIn("Stop-Process -Id $netplayLobby.Id", script)
        self.assertIn("Remove-TestRun $runRoot", script)

    def test_workflow_routes_smoke_and_preserves_full_steps(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        browser = workflow.split("  wasm-browser:\n", 1)[1].split("  fuzz-check:\n", 1)[0]
        self.assertIn("needs: browser-netplay-gate", browser)
        self.assertIn("if: needs.browser-netplay-gate.outputs.suite != 'skip'", browser)
        steps = browser.split("      - name:")
        commands = [step for step in steps if "run: ./scripts/test-wasm-browser-speculation.ps1" in step]
        self.assertEqual(len(commands), 6)
        for step in commands:
            expected = "smoke" if "-NetplaySmoke" in step else "full"
            self.assertIn(f"if: needs.browser-netplay-gate.outputs.suite == '{expected}'", step)
        self.assertIn("fetch-depth: 0", workflow.split("  optimized-build-helper:", 1)[0])

    def test_cli_writes_safe_workflow_output(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            with patch("sys.argv", [
                "browser_netplay_gate.py", "--event-name", "schedule",
                "--event-path", "missing", "--output", str(output),
            ]):
                gate.main()
            self.assertEqual(output.read_text(), "suite=full\n")


if __name__ == "__main__":
    unittest.main()
