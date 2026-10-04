import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("pages_sources", Path(__file__).parents[1] / "pages_sources.py")
pages = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pages)


def release(tag="v0.4.0", date="2026-09-01T12:00:00Z", **changes):
    value = {"tag_name": tag, "published_at": date, "draft": False, "prerelease": False,
             "assets": [{"name": pages.FLATPAK_ARCHIVE}, {"name": "SHA256SUMS.txt"}]}
    value.update(changes)
    return value


class PagesSourceTests(unittest.TestCase):
    def test_selects_latest_published_emulator_without_lobby_or_draft(self):
        values = [release(), release("v0.5.0", "2026-10-01T12:00:00Z"),
                  release("lobby-v0.1.0", "2026-10-04T12:00:00Z"),
                  release("v0.6.0", "2026-10-04T12:00:00Z", draft=True),
                  release("v0.7.0", "2026-10-04T12:00:00Z", prerelease=True)]
        self.assertEqual(pages.select_stable(values), "v0.5.0")

    def test_missing_latest_artifact_fails_instead_of_falling_back(self):
        with self.assertRaises(ValueError):
            pages.select_stable([release(), release("v0.5.0", "2026-10-01T12:00:00Z", assets=[])])
        with self.assertRaises(ValueError):
            pages.select_stable([release("lobby-v0.1.0")])

    def test_resolves_both_sources_to_exact_commit_pins(self):
        with patch.object(pages, "github_json", side_effect=[[[release()]], {"sha": "a" * 40}, {"sha": "b" * 40}]) as api:
            self.assertEqual(pages.resolve("Zeffuro/zeff-boy"), {
                "nightly_sha": "a" * 40, "stable_sha": "b" * 40, "stable_tag": "v0.4.0"})
            self.assertEqual(api.call_args_list[-1].args[0], "repos/Zeffuro/zeff-boy/commits/v0.4.0")
        for value in ["master", "v0.4.0", "a" * 39, "a" * 40 + "\n", None]:
            with self.assertRaises(ValueError):
                pages.checked_sha(value)

    def test_flatpak_requires_exact_checksum_and_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            archive = directory / pages.FLATPAK_ARCHIVE
            archive.write_bytes(b"signed repository fixture")
            checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
            sums = directory / "SHA256SUMS.txt"
            sums.write_text(f"{checksum}  {pages.FLATPAK_ARCHIVE}\n", encoding="utf-8")
            pages.verify_flatpak(directory)
            archive.write_bytes(b"tampered")
            with self.assertRaises(ValueError):
                pages.verify_flatpak(directory)
            sums.write_text(f"{checksum}  {pages.FLATPAK_ARCHIVE}\n" * 2, encoding="utf-8")
            with self.assertRaises(ValueError):
                pages.verify_flatpak(directory)


if __name__ == "__main__":
    unittest.main()
