import importlib.util
import os
import platform
import sys
import tempfile
import unittest
from pathlib import Path


DRIVER_PATH = Path(__file__).parents[1] / "build-optimized.py"
DRIVER_SPEC = importlib.util.spec_from_file_location("build_optimized", DRIVER_PATH)
DRIVER = importlib.util.module_from_spec(DRIVER_SPEC)
DRIVER_SPEC.loader.exec_module(DRIVER)


@unittest.skipUnless(sys.platform == "darwin", "requires the macOS SDK")
class MacosCompilerIntegrationTests(unittest.TestCase):
    def test_real_sdk_and_generated_wrappers_compile_sdk_headers(self):
        architecture = {"arm64": "aarch64"}.get(platform.machine(), platform.machine())
        host = f"{architecture}-apple-darwin"
        normalized_host = host.replace("-", "_")
        environment = os.environ.copy()
        for name in (
            "CC",
            "CXX",
            f"CC_{host}",
            f"CXX_{host}",
            f"CC_{normalized_host}",
            f"CXX_{normalized_host}",
            "TARGET_CC",
            "TARGET_CXX",
        ):
            environment.pop(name, None)

        repository = DRIVER_PATH.parents[1]
        with tempfile.TemporaryDirectory() as temp_dir:
            details = DRIVER.configure_macos_c_wrappers(
                Path(temp_dir), host, environment, repository
            )

            sdk_path = Path(details["c"]["sdk_path"])
            self.assertTrue(sdk_path.is_dir())
            self.assertEqual(details["cxx"]["sdk_path"], str(sdk_path))
            self.assertEqual(Path(environment["CC"]).name, "clang")
            self.assertEqual(Path(environment["CXX"]).name, "clang++")
            self.assertTrue(Path(environment["CC"]).is_file())
            self.assertTrue(Path(environment["CXX"]).is_file())


if __name__ == "__main__":
    unittest.main()
