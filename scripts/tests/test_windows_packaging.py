import importlib.util
import fnmatch
import re
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).parents[2]
GENERATOR_PATH = ROOT / "scripts" / "generate-winget.py"
GENERATOR_SPEC = importlib.util.spec_from_file_location("generate_winget", GENERATOR_PATH)
GENERATOR = importlib.util.module_from_spec(GENERATOR_SPEC)
GENERATOR_SPEC.loader.exec_module(GENERATOR)


class WindowsPackagingTests(unittest.TestCase):
    def test_winget_manifest_targets_the_per_user_inno_installer(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            output_root = Path(temp_dir)
            original_args = GENERATOR.parse_args
            try:
                GENERATOR.parse_args = lambda: type(
                    "Args", (), {"version": "0.4.0", "output_root": output_root, "sha256": "a" * 64}
                )()
                GENERATOR.main()
            finally:
                GENERATOR.parse_args = original_args

            manifest = (
                output_root
                / "manifests"
                / "z"
                / "Zeffuro"
                / "ZeffBoy"
                / "0.4.0"
                / "Zeffuro.ZeffBoy.installer.yaml"
            ).read_text(encoding="utf-8")
            self.assertIn("InstallerType: inno", manifest)
            self.assertIn("Scope: user", manifest)
            self.assertIn("UpgradeBehavior: install", manifest)
            self.assertIn("ProductCode: '{C2417DE7-B9ED-4BE0-AB8B-74873C3B0C49}_is1'", manifest)
            self.assertIn("zeff-boy-v0.4.0-x86_64-pc-windows-msvc-setup.exe", manifest)
            self.assertNotIn("NestedInstallerType", manifest)

    def test_release_upload_includes_the_setup_executable(self):
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        match = re.search(
            r"^\s+path: (zeff-boy-\$\{\{ github\.ref_name \}\}-\$\{\{ matrix\.target \}\}\*)$",
            workflow,
            re.MULTILINE,
        )
        self.assertIsNotNone(match)
        pattern = match.group(1).replace("${{ github.ref_name }}", "v0.4.0").replace(
            "${{ matrix.target }}", "x86_64-pc-windows-msvc"
        )
        setup = "zeff-boy-v0.4.0-x86_64-pc-windows-msvc-setup.exe"
        self.assertTrue(fnmatch.fnmatchcase(setup, pattern))


if __name__ == "__main__":
    unittest.main()
