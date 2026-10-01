import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).parents[1] / "build-optimized.py"
SPEC = importlib.util.spec_from_file_location("windows_pgo", SCRIPT)
DRIVER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DRIVER)


class WindowsPgoPhaseTests(unittest.TestCase):
    def test_actual_driver_keeps_static_runtime_across_builds(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "evidence"
            target = root / "target"
            host = "x86_64-pc-windows-msvc"
            primary = target / host / "release" / "zeff-boy.exe"
            primary.parent.mkdir(parents=True)
            fixture = root / "fixture.gba"
            fixture.write_bytes(b"fixture")
            fixtures = [{"id": core, "core_crate": core, "path": fixture,
                         "frames": 1} for core in sorted(DRIVER.REQUIRED_CORES)]
            calls = {}

            def execute(command, repository, environment, log, *args, **kwargs):
                calls[log.name] = environment.get("CARGO_ENCODED_RUSTFLAGS", "")
                if command[:2] == ["cargo", "build"]:
                    primary.write_bytes(log.name.encode())
                if log.name == "merge.log":
                    Path(command[command.index("-o") + 1]).write_bytes(b"profile")
                if log.name.startswith("training-"):
                    (output / "profiles" / "test.profraw").write_bytes(b"raw")
                if log.name == "coverage.log":
                    return " ".join(DRIVER.REQUIRED_CORES | {"zeff_coleco_core"})
                if "--screenshot" in command:
                    Path(command[command.index("--screenshot") + 1]).write_bytes(b"png")
                    return "frames=1"
                if "coleco" in log.name:
                    return "{}"
                return ""

            with mock.patch.dict(os.environ, {}, clear=True), \
                 mock.patch.object(DRIVER, "tools", return_value={"host": host,
                                   "profdata": "llvm-profdata"}), \
                 mock.patch.object(DRIVER, "fingerprint", return_value={}), \
                 mock.patch.object(DRIVER, "reject_native_config_rustflags"), \
                 mock.patch.object(DRIVER, "configure_macos_c_wrappers"), \
                 mock.patch.object(DRIVER, "corpus", return_value=fixtures), \
                 mock.patch.object(DRIVER, "execute", side_effect=execute):
                self.assertEqual(DRIVER.main([
                    "--artifact", "desktop", "--output-dir", str(output),
                    "--target-dir", str(target), "--verify-frames", "1",
                ]), 0)

            static = "-Ctarget-feature=+crt-static"
            for phase in ["corpus.log", "control.log", "instrumented.log", "optimized.log"]:
                self.assertIn(static, calls[phase].split("\x1f"), phase)
            self.assertIn("-Cprofile-generate=.", calls["instrumented.log"])
            self.assertIn("-Cprofile-use=", calls["optimized.log"])
            self.assertNotIn("-Cprofile-", calls["control.log"])
            report = json.loads((output / "manifest.json").read_text())
            self.assertEqual(report["base_rustflags"], [static])


if __name__ == "__main__":
    unittest.main()
