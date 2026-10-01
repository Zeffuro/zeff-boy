#!/usr/bin/env python3
"""Reject Windows release artifacts that require the Visual C++ runtime DLLs."""
import argparse
from pathlib import Path
import re
import subprocess


RUNTIME_DLL = re.compile(
    r"(?:vcruntime\d+.*|msvcp\d+.*|msvcr\d+.*|concrt\d+.*|"
    r"ucrtbase|api-ms-win-crt-.+)\.dll", re.IGNORECASE
)


def validate_imports(output):
    if "Format: COFF-x86-64" not in output:
        raise RuntimeError("expected an x64 Windows PE artifact")
    imports = sorted(set(re.findall(r"^\s+Name: (\S+\.dll)\s*$", output,
                                    re.MULTILINE | re.IGNORECASE)))
    if not imports:
        raise RuntimeError("no DLL imports found; runtime linkage was not verified")
    forbidden = [name for name in imports if RUNTIME_DLL.fullmatch(name)]
    if forbidden:
        raise RuntimeError("dynamic C runtime imports: " + ", ".join(forbidden))
    return imports


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("artifact", type=Path)
    args = parser.parse_args()
    target_libdir = subprocess.check_output(
        ["rustc", "--print", "target-libdir"], text=True
    ).strip()
    reader = Path(target_libdir).parent / "bin" / "llvm-readobj.exe"
    if not reader.is_file():
        raise RuntimeError("llvm-readobj missing; install llvm-tools-preview")
    output = subprocess.check_output(
        [str(reader), "--coff-imports", str(args.artifact)], text=True
    )
    imports = validate_imports(output)
    print(f"Static C runtime verified: {args.artifact}")
    print("DLL imports: " + ", ".join(imports))


if __name__ == "__main__":
    main()
