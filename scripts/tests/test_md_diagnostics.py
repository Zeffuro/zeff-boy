import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.parse
import zipfile


spec = importlib.util.spec_from_file_location(
    "md_diagnostics", Path(__file__).parents[1] / "fetch-md-diagnostics.py"
)
diagnostics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(diagnostics)


def archive(*entries):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as target:
        for name, payload in entries:
            if isinstance(name, str):
                entry = zipfile.ZipInfo(name)
                entry.filename = name
                name = entry
            target.writestr(name, payload)
    return output.getvalue()


class DownloadBoundsTests(unittest.TestCase):
    def test_response_at_limit_is_admitted_but_extra_byte_is_rejected(self):
        with patch.object(diagnostics, "LIMIT", 3):
            self.assertEqual(diagnostics.read_response(io.BytesIO(b"abc")), b"abc")
            with self.assertRaisesRegex(ValueError, "oversized diagnostic download"):
                diagnostics.read_response(io.BytesIO(b"abcd"))

    def test_verified_cache_is_reused_and_corruption_never_redownloaded(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "archive.zip"
            path.write_bytes(b"correct")
            with patch.object(diagnostics, "get") as download:
                self.assertEqual(diagnostics.cached(path, diagnostics.digest(b"correct"), download),
                                 b"correct")
                with self.assertRaisesRegex(ValueError, "diagnostic hash mismatch"):
                    diagnostics.cached(path, diagnostics.digest(b"other"), download)
                download.assert_not_called()
            self.assertEqual(path.read_bytes(), b"correct")

    def test_failed_download_hash_does_not_create_cache_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "archive.zip"
            with self.assertRaisesRegex(ValueError, "diagnostic hash mismatch"):
                diagnostics.cached(path, diagnostics.digest(b"correct"), lambda: b"wrong")
            self.assertFalse(path.exists())

    def test_oversized_cache_and_download_callback_are_bounded(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(diagnostics, "LIMIT", 3):
            path = Path(directory) / "archive.zip"
            path.write_bytes(b"abcd")
            with self.assertRaisesRegex(ValueError, "oversized"):
                diagnostics.cached(path, diagnostics.digest(b"abcd"), lambda: b"unused")
            absent = Path(directory) / "absent.zip"
            with self.assertRaisesRegex(ValueError, "oversized"):
                diagnostics.cached(absent, diagnostics.digest(b"abcd"), lambda: b"abcd")
            self.assertFalse(absent.exists())


class ArchiveExtractionTests(unittest.TestCase):
    def test_pinned_prefix_and_existing_identical_members_are_reusable(self):
        data = archive(("pinned/", b""), ("pinned/data/rom.bin", b"ROM"))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "extracted"
            diagnostics.extract(data, root, "pinned")
            diagnostics.extract(data, root, "pinned")
            self.assertEqual((root / "data/rom.bin").read_bytes(), b"ROM")
            self.assertFalse((root / "pinned").exists())

    def test_rejects_traversal_absolute_windows_and_alternate_stream_paths(self):
        for name in ("../escape.bin", "/escape.bin", "C:/escape.bin",
                     "data\\escape.bin", "rom.bin:stream", "pinned/../../escape.bin"):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                with self.assertRaisesRegex(ValueError, "unsafe diagnostic archive path"):
                    diagnostics.extract(archive((name, b"payload")), Path(directory) / "out")
                self.assertFalse((Path(directory) / "escape.bin").exists())

    def test_rejects_symlink_archive_member(self):
        entry = zipfile.ZipInfo("link")
        entry.create_system = 3
        entry.external_attr = 0o120777 << 16
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "contains symlink"):
                diagnostics.extract(archive((entry, b"../outside")), Path(directory))
            self.assertFalse((Path(directory) / "link").exists())

    def test_wrong_prefix_and_invalid_zip_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                diagnostics.extract(archive(("other/rom.bin", b"ROM")), Path(directory), "pinned")
            with self.assertRaises(zipfile.BadZipFile):
                diagnostics.extract(b"not a zip", Path(directory))

    def test_total_uncompressed_bound_precedes_extraction(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(diagnostics, "LIMIT", 1):
            with self.assertRaisesRegex(ValueError, "oversized diagnostic archive"):
                diagnostics.extract(archive(("one", b"abc"), ("two", b"de")), Path(directory))
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_existing_different_source_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.asm").write_bytes(b"original")
            with self.assertRaisesRegex(ValueError, "existing diagnostic source differs"):
                diagnostics.extract(archive(("source.asm", b"replacement")), root)
            self.assertEqual((root / "source.asm").read_bytes(), b"original")


class OfficialDownloadTests(unittest.TestCase):
    suite = {"page": "https://publisher.itch.io/suite", "upload_id": 123}

    def test_csrf_metadata_supports_content_value_and_html_entities(self):
        for html, expected in (
            ('<meta name="csrf_token" content="one&amp;two">', "one&two"),
            ('<meta value="value-token" name="csrf_token">', "value-token"),
            ('<meta name="csrf_token" content="" value="fallback">', "fallback"),
        ):
            with self.subTest(html=html):
                parser = diagnostics.CsrfParser()
                parser.feed(html)
                self.assertEqual(parser.token, expected)
        self.assertIsNone(diagnostics.CsrfParser().token)

    def test_anonymous_token_post_and_file_get_share_one_opener(self):
        with patch.object(diagnostics.urllib.request, "build_opener") as factory:
            opener = factory.return_value
            opener.open.side_effect = [
                io.BytesIO(b'<meta name="csrf_token" value="a&b">'),
                io.BytesIO(json.dumps({"url": "https://cdn.example/file?key=value"}).encode()),
                io.BytesIO(b"ROM"),
            ]
            self.assertEqual(diagnostics.itch_file(self.suite), b"ROM")
            self.assertEqual(opener.open.call_count, 3)
            request = opener.open.call_args_list[1].args[0]
            self.assertEqual(request.get_method(), "POST")
            self.assertEqual(request.full_url, self.suite["page"] + "/file/123")
            self.assertEqual(urllib.parse.parse_qs(request.data.decode()), {"csrf_token": ["a&b"]})
            self.assertEqual(request.get_header("Referer"), self.suite["page"])
            self.assertEqual(opener.open.call_args_list[2].args[0], "https://cdn.example/file?key=value")

    def test_missing_token_fails_before_post(self):
        with patch.object(diagnostics.urllib.request, "build_opener") as factory:
            opener = factory.return_value
            opener.open.return_value = io.BytesIO(b'<meta name="unrelated" value="token">')
            with self.assertRaisesRegex(ValueError, "no anonymous request token"):
                diagnostics.itch_file(self.suite)
            self.assertEqual(opener.open.call_count, 1)

    def test_missing_non_https_malformed_and_non_string_file_urls_fail_before_get(self):
        for url in (None, "http://cdn.example/file", "file:///local", "https:///missing-host", 123):
            with self.subTest(url=url), patch.object(diagnostics.urllib.request, "build_opener") as factory:
                opener = factory.return_value
                opener.open.side_effect = [io.BytesIO(b'<meta name="csrf_token" content="token">'),
                                          io.BytesIO(json.dumps({"url": url}).encode())]
                with self.assertRaisesRegex(ValueError, "HTTPS file URL"):
                    diagnostics.itch_file(self.suite)
                self.assertEqual(opener.open.call_count, 2)


class BuildCommandTests(unittest.TestCase):
    def test_pinned_build_uses_argument_lists_bounds_and_exact_rom_hash(self):
        tools = archive(("asw.exe", b"assembler"), ("p2bin.exe", b"converter"))
        bcd = {"assembler": {"sha256": diagnostics.digest(tools), "url": "https://tools.example/as.zip"},
               "rom_sha256": diagnostics.digest(b"ROM"), "table_sha256": diagnostics.digest(b"TABLE")}
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            source = cache / "source"
            source.mkdir()
            (source / "bcd-verifier.bin").write_bytes(b"ROM")
            (source / "data").mkdir()
            (source / "data/bcd-table.bin").write_bytes(b"TABLE")
            with patch.object(diagnostics.shutil, "which", return_value="g++"), \
                    patch.object(diagnostics, "get", return_value=tools), \
                    patch.object(diagnostics.subprocess, "run") as run:
                self.assertEqual(diagnostics.build_bcd(bcd, source, cache), b"ROM")
                self.assertEqual(run.call_count, 4)
                for call in run.call_args_list:
                    self.assertIsInstance(call.args[0], list)
                    self.assertEqual(call.kwargs["cwd"], source)
                    self.assertTrue(call.kwargs["check"])
                    self.assertEqual(call.kwargs["timeout"], 60)
                    self.assertNotIn("shell", call.kwargs)
                self.assertEqual(run.call_args_list[-1].args[0][1:], ["-r", "0-$", "bcd-verifier.p"])
                bcd["rom_sha256"] = diagnostics.digest(b"different")
                with self.assertRaisesRegex(ValueError, "built BCD ROM"):
                    diagnostics.build_bcd(bcd, source, cache)

    def test_matching_rom_cannot_mask_corrupted_generated_table(self):
        tools = archive(("asw.exe", b"assembler"), ("p2bin.exe", b"converter"))
        bcd = {"assembler": {"sha256": diagnostics.digest(tools), "url": "https://tools.example/as.zip"},
               "rom_sha256": diagnostics.digest(b"ROM"), "table_sha256": diagnostics.digest(b"TABLE")}
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            source = cache / "source"
            (source / "data").mkdir(parents=True)
            (source / "bcd-verifier.bin").write_bytes(b"ROM")
            (source / "data/bcd-table.bin").write_bytes(b"altered table")
            with patch.object(diagnostics.shutil, "which", return_value="g++"), \
                    patch.object(diagnostics, "get", return_value=tools), \
                    patch.object(diagnostics.subprocess, "run") as run:
                with self.assertRaisesRegex(ValueError, "built BCD table"):
                    diagnostics.build_bcd(bcd, source, cache)
                self.assertEqual(run.call_count, 4)

    def test_missing_compiler_fails_before_tool_download(self):
        with patch.object(diagnostics.shutil, "which", return_value=None), \
                patch.object(diagnostics, "get") as download:
            with self.assertRaisesRegex(ValueError, "requires g\\+\\+"):
                diagnostics.build_bcd({}, Path("unused"), Path("unused"))
            download.assert_not_called()


if __name__ == "__main__":
    unittest.main()
