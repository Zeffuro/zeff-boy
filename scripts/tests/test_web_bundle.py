import importlib.util
import json
from pathlib import Path
import re
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("web_bundle", Path(__file__).parents[1] / "web_bundle.py")
web_bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(web_bundle)


class BundleTests(unittest.TestCase):
    def fixture(self, root):
        (root / "app.js").write_text("import {save} from './snippets/save.js';")
        (root / "app_bg.wasm").write_bytes(b"wasm-module")
        (root / "snippets").mkdir()
        (root / "snippets/save.js").write_text("export function save() {}")
        (root / "index.html").write_text(
            "import init, * as bindings from '/stable/app.js';\n"
            "const wasm = await init({ module_or_path: '/stable/app_bg.wasm' });"
        )

    def manifest(self, root):
        web_bundle.bind_bundle(root)
        html = (root / "index.html").read_text()
        return json.loads(re.search(r"const manifest = (.*);", html).group(1)), html

    def test_loaded_bytes_and_all_glue_modules_are_bound(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            manifest, html = self.manifest(root)
            self.assertEqual(set(manifest["files"]), {"app.js", "app_bg.wasm", "snippets/save.js"})
            self.assertEqual(len(manifest["loader"]), 64)
            self.assertIn("module_or_path: bytes.get(\"app_bg.wasm\")", html)
            self.assertNotIn("await init({ module_or_path:", html)
            self.assertIn("cache: 'no-store'", html)

    def test_module_or_glue_change_changes_identity(self):
        for asset in ["app.js", "app_bg.wasm", "snippets/save.js"]:
            with self.subTest(asset=asset), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.fixture(root)
                before, _ = self.manifest(root)
                (root / asset).write_bytes((root / asset).read_bytes() + b"changed")
                (root / "index.html").write_text(
                    "import init, * as bindings from '/stable/app.js';\n"
                    "const wasm = await init({ module_or_path: '/stable/app_bg.wasm' });"
                )
                after, _ = self.manifest(root)
                self.assertNotEqual(before, after)

    def test_missing_asset_or_unknown_loader_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            (root / "app_bg.wasm").unlink()
            with self.assertRaises(FileNotFoundError):
                web_bundle.bind_bundle(root)
            (root / "index.html").write_text("unknown loader")
            with self.assertRaises(ValueError):
                web_bundle.bind_bundle(root)


if __name__ == "__main__":
    unittest.main()
