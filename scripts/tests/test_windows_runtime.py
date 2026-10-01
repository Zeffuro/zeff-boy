import importlib.util
from pathlib import Path
import unittest


SCRIPT = Path(__file__).parents[1] / "verify-windows-runtime.py"
SPEC = importlib.util.spec_from_file_location("windows_runtime", SCRIPT)
RUNTIME = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNTIME)


class WindowsRuntimeTests(unittest.TestCase):
    def test_accepts_windows_system_dependencies(self):
        output = "Format: COFF-x86-64\nImport {\n  Name: KERNEL32.dll\n}\n"
        self.assertEqual(RUNTIME.validate_imports(output), ["KERNEL32.dll"])

    def test_rejects_c_runtime_imports_including_delayed_imports(self):
        for dll in ["VCRUNTIME140.dll", "vcruntime140_1.dll", "MSVCP140.dll",
                    "MSVCR120.dll", "CONCRT140.dll", "ucrtbase.dll",
                    "api-ms-win-crt-runtime-l1-1-0.dll"]:
            for section in ["Import", "DelayImport"]:
                with self.subTest(dll=dll, section=section):
                    output = f"Format: COFF-x86-64\n{section} {{\n  Name: {dll}\n}}\n"
                    with self.assertRaisesRegex(RuntimeError, "dynamic C runtime"):
                        RUNTIME.validate_imports(output)

    def test_rejects_unrecognized_or_empty_inspection(self):
        for output in ["", "Format: ELF64\n", "Format: COFF-x86-64\n"]:
            with self.subTest(output=output):
                with self.assertRaises(RuntimeError):
                    RUNTIME.validate_imports(output)


if __name__ == "__main__":
    unittest.main()
