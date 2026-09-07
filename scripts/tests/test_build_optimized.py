import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock


DRIVER_PATH = Path(__file__).parents[1] / "build-optimized.py"
DRIVER_SPEC = importlib.util.spec_from_file_location("build_optimized", DRIVER_PATH)
DRIVER = importlib.util.module_from_spec(DRIVER_SPEC)
DRIVER_SPEC.loader.exec_module(DRIVER)


class BuildOptimizedTests(unittest.TestCase):
    def test_cargo_warning_summaries_are_not_profile_diagnostics(self):
        summaries = [
            'warning: `zeff-pgo-corpus` (lib) generated 2 warnings',
            'warning: `profile-helper` (lib) generated 1 warning',
            'warning: `zeff-pgo-corpus` (lib) generated 2 warnings (2 duplicates)',
            'warning: `profile-helper` (lib test) generated 1 warning (1 duplicate)',
            'warning: `profile-helper` (bin "profile-tool") generated 12 warnings (11 duplicates)',
        ]
        for line in summaries:
            with self.subTest(line=line):
                result = DRIVER.classify_profile_diagnostics("\x1b[33m" + line + "\x1b[0m")
                self.assertEqual(result, {"missing_function_count": 0, "rejected_count": 0})

    def test_cargo_summary_lookalikes_do_not_hide_profile_errors(self):
        lines = [
            'warning: `zeff-pgo-corpus` (lib) generated 2 warnings: hash mismatch',
            'warning: `zeff-pgo-corpus` (lib) generated 2 warnings (profile invalid)',
            'warning: `profile-helper` (lib) generated 1 warning (1 duplicates)',
            'warning: `profile-helper` (lib) generated 2 warnings (duplicates)',
            'error: `zeff-pgo-corpus` (lib) generated 2 warnings',
        ]
        for line in lines:
            with self.subTest(line=line):
                result = DRIVER.classify_profile_diagnostics(line)
                self.assertEqual(result["rejected_count"], 1)
        combined = ('warning: `zeff-pgo-corpus` (lib) generated 2 warnings\n'
                    'warning: malformed instrumentation profile data')
        self.assertEqual(DRIVER.classify_profile_diagnostics(combined)["rejected_count"], 1)

    def test_missing_function_diagnostic_is_recorded_not_a_hash_mismatch(self):
        line = (
            "warning: no profile data available for function _RNvXfmt "
            "Hash = 123456 up to 0 count discarded"
        )
        for text in (line, "\x1b[33m" + line + "\x1b[0m"):
            self.assertEqual(DRIVER.classify_profile_diagnostics(text), {
                "missing_function_count": 1, "rejected_count": 0,
            })

    def test_profile_mismatch_and_unexpected_diagnostics_remain_fatal(self):
        messages = [
            "function control flow change detected (hash mismatch)",
            "function basic block count change detected (counter mismatch)",
            "function value site count change detected (counter mismatch)",
            "function bitmap size change detected (bitmap size mismatch)",
            "malformed instrumentation profile data",
            "invalid instrumentation profile data (bad magic)",
            "unsupported instrumentation profile format version",
            "Inconsistent number of counts in foo: the profile may be stale",
            "unexpected PGO optimization problem",
            "no profile data available for function foo Hash = 1 up to 1 count discarded",
            "no profile data available for function foo Hash = 1",
        ]
        for message in messages:
            with self.subTest(message=message):
                line = "warning: " + message
                if "change detected" in message:
                    line += " foo Hash = 123 up to 0 count discarded"
                result = DRIVER.classify_profile_diagnostics("\x1b[33m" + line + "\x1b[0m")
                self.assertEqual(result["missing_function_count"], 0)
                self.assertEqual(result["rejected_count"], 1)

    def test_missing_record_does_not_hide_another_profile_warning(self):
        text = ("warning: no profile data available for function foo Hash = 1 "
                "up to 0 count discarded\nwarning: malformed instrumentation profile data")
        self.assertEqual(DRIVER.classify_profile_diagnostics(text), {
            "missing_function_count": 1, "rejected_count": 1,
        })

    def test_generate_profile_flag_has_explicit_stable_value(self):
        self.assertEqual(DRIVER.profile_generate_flags(), "-Cprofile-generate=.")

    def test_c_pgo_filter_preserves_argument_vector(self):
        args = ["-O2", "source file.c", "-fprofile-generate=.", "-DVALUE=a b",
                "-fprofile-use", "profile data", "-o", "output file.o"]
        expected = ["-O2", "source file.c", "-DVALUE=a b", "profile data",
                    "-o", "output file.o"]
        self.assertEqual(DRIVER.strip_c_pgo_args(args), expected)

    def test_c_pgo_filter_ignores_similar_flags(self):
        args = ["-fprofile-instr-generate", "-Wl,-fprofile-use=x",
                "-fprofile-generate-other"]
        self.assertEqual(DRIVER.strip_c_pgo_args(args), args)

    def test_macos_compiler_args_add_sdk_once_and_strip_pgo(self):
        args = ["-O2", "source.c", "-fprofile-generate=."]
        self.assertEqual(
            DRIVER.macos_compiler_args(args, "/SDK Path"),
            ["-isysroot", "/SDK Path", "-O2", "source.c"],
        )
        existing = ["-isysroot", "/Existing SDK", "source.c"]
        self.assertEqual(
            DRIVER.macos_compiler_args(existing, "/SDK Path"), existing
        )

    def test_macos_wrappers_are_executable_python_and_recorded(self):
        def compiler_output(command, _environment):
            if "--show-sdk-path" in command:
                return str(root)
            if "clang++" in command:
                return "/usr/bin/clang++"
            if "--find" in command:
                return "/usr/bin/clang"
            return "Apple clang 17"

        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            environment = {}
            with mock.patch.object(
                DRIVER, "output", side_effect=compiler_output
            ), mock.patch.object(DRIVER, "preflight_macos_wrapper") as preflight:
                details = DRIVER.configure_macos_c_wrappers(
                    root, "aarch64-apple-darwin", environment, root
                )

            c_wrapper = Path(environment["CC"])
            self.assertEqual(environment["CC"], details["c"]["wrapper"])
            compile(c_wrapper.read_text(), str(c_wrapper), "exec")
            wrapper_args = [str(c_wrapper), "-fprofile-use=profile", "source.c",
                            "-fprofile-generate", "-O2"]
            with mock.patch("os.execv") as execv, mock.patch("sys.argv", wrapper_args):
                exec(c_wrapper.read_text(), {"__name__": "__main__"})
            execv.assert_called_once_with(
                "/usr/bin/clang",
                ["/usr/bin/clang", "-isysroot", str(root), "source.c", "-O2"],
            )
            self.assertEqual(environment["MACOSX_DEPLOYMENT_TARGET"], "11.0")
            self.assertEqual(preflight.call_count, 2)
            if os.name != "nt":
                self.assertTrue(c_wrapper.stat().st_mode & 0o100)

    def test_cargo_config_allows_wasm_only_and_rejects_native_flags(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            config = root / ".cargo" / "config.toml"
            config.parent.mkdir()
            config.write_text(
                '[target.wasm32-unknown-unknown]\n'
                'rustflags=["-C","target-feature=-bulk-memory"]\n'
            )
            environment = {"CARGO_HOME": str(root / "home")}
            DRIVER.reject_native_config_rustflags(
                root, "x86_64-pc-windows-msvc", environment
            )
            config.write_text('[build]\nrustflags=["-Ctarget-cpu=native"]\n')
            with self.assertRaises(RuntimeError):
                DRIVER.reject_native_config_rustflags(
                    root, "x86_64-pc-windows-msvc", environment
                )

    def test_corpus(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            rom = root / "a.gba"
            rom.write_bytes(b"x")
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({
                "schema": 1,
                "fixtures": [{
                    "id": "a", "system": "GBA", "path": "a.gba",
                    "sha256": DRIVER.digest(rom), "frames": 2,
                }],
            }))
            self.assertEqual(
                DRIVER.corpus(manifest)[0]["core_crate"], "zeff_gba_core"
            )

    def test_escape_rejected(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            manifest = Path(temp_dir) / "manifest.json"
            manifest.write_text(json.dumps({
                "schema": 1,
                "fixtures": [{
                    "id": "a", "system": "GBA", "path": "../a",
                    "sha256": "0", "frames": 1,
                }],
            }))
            with self.assertRaises(RuntimeError):
                DRIVER.corpus(manifest)

    def test_unsafe_id_rejected(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            rom = root / "a.gba"
            rom.write_bytes(b"x")
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({
                "schema": 1,
                "fixtures": [{
                    "id": "../bad", "system": "GBA", "path": "a.gba",
                    "sha256": DRIVER.digest(rom), "frames": 1,
                }],
            }))
            with self.assertRaises(RuntimeError):
                DRIVER.corpus(manifest)

    def test_semantics(self):
        summary_tail = (
            " callback_payload_hashing=true state_roundtrip=true"
            " repeated_state_hashes_match=true repeated_video_hashes_match=true"
            " repeated_audio_hashes_match=true repeated_callback_counts_match=true"
            " continuation_match=true video_sha256=v audio_sha256=a"
            " serialize_sha256=s save_ram_sha256=r"
            " save_ram_post_roundtrip_sha256=p"
            " continuation_uninterrupted_video_sha256=uv"
            " continuation_uninterrupted_audio_sha256=ua"
            " continuation_uninterrupted_state_sha256=us"
            " continuation_restored_video_sha256=rv"
            " continuation_restored_audio_sha256=ra"
            " continuation_restored_state_sha256=rs"
        )
        first = DRIVER.semantics("runs=2 fps_p50=1 video_sha256=a" + summary_tail)
        second = DRIVER.semantics("runs=2 fps_p50=2 video_sha256=a" + summary_tail)
        self.assertEqual(first, second)

        changed = ("runs=2" + summary_tail).replace(
            "serialize_sha256=s", "serialize_sha256=t"
        )
        self.assertNotEqual(
            DRIVER.semantics("runs=2" + summary_tail), DRIVER.semantics(changed)
        )

    def test_missing_or_unhashed_summary_rejected(self):
        with self.assertRaises(RuntimeError):
            DRIVER.semantics("")
        with self.assertRaises(RuntimeError):
            DRIVER.semantics("runs=2 callback_payload_hashing=false")

        incomplete_summary = (
            "runs=2 callback_payload_hashing=true state_roundtrip=true"
            " repeated_state_hashes_match=true repeated_video_hashes_match=true"
            " repeated_audio_hashes_match=true repeated_callback_counts_match=true"
        )
        with self.assertRaises(RuntimeError):
            DRIVER.semantics(incomplete_summary)


if __name__ == "__main__":
    unittest.main()
