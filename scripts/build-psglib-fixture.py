#!/usr/bin/env python3
"""Build the pinned two-song PSGlib SMS fixture without changing global tools."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "fixtures" / "psglib"
UPSTREAM_URL = "https://github.com/sverx/devkitSMS.git"
UPSTREAM_COMMIT = "f433a35d26337efe8b18a72fdcc9ac1ef0713f74"
PSGLIB_FILES = (
    "PSGlib.c",
    "PSGAttenuation.c",
    "PSGPlayLoops.c",
    "PSGRestoreVolumes.c",
    "PSGResume.c",
)
PSGLIB_INPUTS = PSGLIB_FILES + ("PSGlib.h", "PSGlib_extern.h")
ROM_SIZE = 0x8000
SEGA_HEADER = 0x7FF0


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command: list[str], cwd: Path) -> str:
    print("+", subprocess.list2cmdline(command))
    completed = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        capture_output=True,
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
    )
    if completed.stdout:
        print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="", file=sys.stderr)
    completed.check_returncode()
    return completed.stdout


def executable(value: str | None, name: str, sibling_to: Path | None = None) -> Path:
    if value:
        candidate = Path(value).expanduser().resolve()
    elif sibling_to:
        candidate = sibling_to.with_name(f"{name}{sibling_to.suffix}")
    else:
        found = shutil.which(name)
        if not found:
            raise SystemExit(f"{name} was not found; pass --{name} PATH")
        candidate = Path(found)
    if not candidate.is_file():
        raise SystemExit(f"{name} executable does not exist: {candidate}")
    return candidate


def upstream(work_dir: Path) -> Path:
    checkout = work_dir / "devkitSMS"
    if not checkout.exists():
        work_dir.mkdir(parents=True, exist_ok=True)
        run(["git", "clone", UPSTREAM_URL, str(checkout)], ROOT)
    actual = run(["git", "rev-parse", "HEAD"], checkout).strip()
    if actual != UPSTREAM_COMMIT:
        run(["git", "fetch", "--tags", "origin"], checkout)
        run(["git", "checkout", "--detach", UPSTREAM_COMMIT], checkout)
        actual = run(["git", "rev-parse", "HEAD"], checkout).strip()
    if actual != UPSTREAM_COMMIT:
        raise SystemExit(f"pinned devkitSMS revision was not available: {actual}")
    dirty = run(["git", "status", "--porcelain", "--untracked-files=all", "--", "PSGlib"], checkout)
    if dirty:
        raise SystemExit("refusing to compile a dirty pinned PSGlib checkout")
    return checkout


def parse_ihx(path: Path) -> bytearray:
    image = bytearray(ROM_SIZE)
    base = 0
    for number, line in enumerate(path.read_text(encoding="ascii").splitlines(), start=1):
        if not line.startswith(":"):
            raise ValueError(f"{path}:{number}: invalid Intel HEX record")
        record = bytes.fromhex(line[1:])
        if not record or len(record) != record[0] + 5 or sum(record) & 0xFF:
            raise ValueError(f"{path}:{number}: invalid Intel HEX checksum")
        length, high, low, kind = record[:4]
        payload = record[4:-1]
        address = base + ((high << 8) | low)
        if kind == 0:
            if address + length > ROM_SIZE:
                raise ValueError(f"{path}:{number}: record lies outside 32 KiB ROM")
            image[address : address + length] = payload
        elif kind == 1:
            break
        elif kind == 4:
            if length != 2:
                raise ValueError(f"{path}:{number}: malformed extended address")
            base = int.from_bytes(payload, "big") << 16
        else:
            raise ValueError(f"{path}:{number}: unsupported Intel HEX record type {kind}")
    return image


def finish_sms(ihx: Path, output: Path) -> None:
    image = parse_ihx(ihx)
    image[SEGA_HEADER : SEGA_HEADER + 16] = b"TMR SEGA\x00\x00\x00\x00\x00\x00\x00\x4c"
    checksum = sum(image[:SEGA_HEADER])
    image[SEGA_HEADER + 10 : SEGA_HEADER + 12] = (checksum & 0xFFFF).to_bytes(2, "little")
    output.write_bytes(image)


def symbol_addresses(map_path: Path, table: bool = False) -> dict[str, int]:
    wanted = ("psglib_song_0", "psglib_song_1", "PSGFrame", "PSGPlay", "PSGLoopFlag")
    wanted += ("table_play", "song_table") if table else ("PSGPlayLoops", "PSGGetStatus")
    found: dict[str, int] = {}
    for line in map_path.read_text(encoding="utf-8", errors="replace").splitlines():
        for name in wanted:
            if re.search(rf"\b_?{name}\b", line):
                numbers = re.findall(r"\b[0-9A-Fa-f]{4,8}\b", line)
                if numbers:
                    found.setdefault(name, int(numbers[0], 16) & 0xFFFF)
    missing = [name for name in wanted if name not in found]
    if missing:
        raise ValueError(f"link map did not expose expected symbols: {', '.join(missing)}")
    return found


def call_sites(image: bytes, target: int) -> list[int]:
    needle = bytes((0xCD, target & 0xFF, target >> 8))
    return [at for at in range(len(image) - 2) if image[at : at + 3] == needle]


def vector_target(image: bytes, offset: int) -> int:
    if image[offset] != 0xC3:
        raise ValueError(f"expected JP vector at 0x{offset:04x}, got 0x{image[offset]:02x}")
    return int.from_bytes(image[offset + 1 : offset + 3], "little")


def build_one(
    output: Path,
    checkout: Path,
    sdcc: Path,
    sdasz80: Path,
    sdar: Path,
    layout: tuple[int, int],
    table_selector: int | None = None,
) -> dict[str, object]:
    output.mkdir(parents=True)
    library = output / "library"
    library.mkdir()
    source_dir = checkout / "PSGlib" / "src"
    objects: list[Path] = []
    for source_name in PSGLIB_FILES:
        source = source_dir / source_name
        object_path = library / f"{source.stem}.rel"
        run(
            [str(sdcc), "-c", "-mz80", "--max-allocs-per-node", "100000", "-o", str(object_path), str(source)],
            library,
        )
        objects.append(object_path)
    library_path = library / "PSGlib.lib"
    run([str(sdar), "r", str(library_path), *map(str, objects)], library)
    crt = output / "fixture-crt.rel"
    app = output / "fixture.rel"
    run([str(sdasz80), "-o", str(crt), str(FIXTURE / "fixture-crt.s")], output)
    extra_objects = []
    if table_selector is None:
        run([str(sdcc), "-c", "-mz80", "-o", str(app), str(FIXTURE / "fixture.c")], output)
    else:
        run([str(sdcc), "-c", "-mz80", f"-DTABLE_SELECTOR={table_selector}",
             "-o", str(app), str(FIXTURE / "table.c")], output)
        table_object = output / "table.rel"
        run([str(sdasz80), "-o", str(table_object), str(FIXTURE / "table.s")], output)
        extra_objects.append(str(table_object))
    ihx = output / "psglib-two-song.ihx"
    run(
        [
            str(sdcc), "-mz80", "--no-std-crt0", "--code-loc", hex(layout[0]), "--data-loc", hex(layout[1]),
            "-o", str(ihx), str(crt), str(app), *extra_objects, str(library_path),
        ],
        output,
    )
    sms = output / "psglib-two-song.sms"
    finish_sms(ihx, sms)
    image = sms.read_bytes()
    symbols = symbol_addresses(ihx.with_suffix(".map"), table_selector is not None)
    return {
        "rom": sms.name,
        "sha256": sha256(sms),
        "byte_len": len(image),
        "symbols": symbols,
        "reset_jump_target": vector_target(image, 0x0006),
        "irq_jump_target": vector_target(image, 0x0038),
        "psg_frame_call_sites": call_sites(image, symbols["PSGFrame"]),
        "psg_play_loops_call_sites": call_sites(image, symbols["PSGPlayLoops"]) if table_selector is None else [],
        **({"table_call_sites": call_sites(image, symbols["table_play"])} if table_selector is not None else {}),
        "source_hashes": {name: sha256(source_dir / name) for name in PSGLIB_INPUTS},
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdcc", help="path to a relocatable SDCC executable")
    parser.add_argument("--sdasz80", help="path to SDCC's sdasz80 executable")
    parser.add_argument("--sdar", help="path to SDCC's sdar executable")
    parser.add_argument("--work-dir", type=Path, default=ROOT / ".tmp" / "psglib-fixture-source")
    parser.add_argument("--out-dir", type=Path, required=True, help="new directory that receives both builds and receipt")
    parser.add_argument("--code-loc", type=lambda value: int(value, 0), default=0x80)
    parser.add_argument("--data-loc", type=lambda value: int(value, 0), default=0xc000)
    parser.add_argument("--table-selector", type=int, choices=[0, 1], help="boot only this entry of the two-song table fixture")
    args = parser.parse_args()
    args.out_dir = args.out_dir.resolve()
    args.work_dir = args.work_dir.resolve()
    if args.out_dir.exists():
        raise SystemExit(f"refusing to overwrite existing output directory: {args.out_dir}")
    if not 0x80 <= args.code_loc <= 0x3000 or not 0xc000 <= args.data_loc <= 0xd000:
        raise SystemExit("fixture layout requires code 0x80..0x3000 and data 0xc000..0xd000")
    sdcc = executable(args.sdcc, "sdcc")
    sdasz80 = executable(args.sdasz80, "sdasz80", sdcc)
    sdar = executable(args.sdar, "sdar", sdcc)
    sdldz80 = executable(None, "sdldz80", sdcc)
    checkout = upstream(args.work_dir)
    args.out_dir.mkdir(parents=True)
    layout = (args.code_loc, args.data_loc)
    first = build_one(args.out_dir / "build-1", checkout, sdcc, sdasz80, sdar, layout, args.table_selector)
    second = build_one(args.out_dir / "build-2", checkout, sdcc, sdasz80, sdar, layout, args.table_selector)
    first_bytes = (args.out_dir / "build-1" / str(first["rom"])).read_bytes()
    second_bytes = (args.out_dir / "build-2" / str(second["rom"])).read_bytes()
    if first_bytes != second_bytes:
        raise SystemExit("two independent fixture builds differ")
    receipt = {
        "schema": "zeff-psglib-fixture/1",
        "layout": {"code": args.code_loc, "data": args.data_loc},
        "upstream": {"url": UPSTREAM_URL, "commit": UPSTREAM_COMMIT, "license": "Unlicense/public domain"},
        "tools": {
            "sdcc": str(sdcc), "sdasz80": str(sdasz80), "sdar": str(sdar), "sdldz80": str(sdldz80),
            "sdcc_version": run([str(sdcc), "--version"], args.out_dir).strip(),
            "sdcc_sha256": sha256(sdcc), "sdasz80_sha256": sha256(sdasz80), "sdar_sha256": sha256(sdar),
            "sdldz80_sha256": sha256(sdldz80),
        },
        "fixture_sources": {name: sha256(FIXTURE / name) for name in (
            ("fixture-crt.s", "fixture.c") if args.table_selector is None
            else ("fixture-crt.s", "table.c", "table.s"))},
        **({"table_selector": args.table_selector} if args.table_selector is not None else {}),
        "oracle": json.loads((FIXTURE / "oracle.json").read_text(encoding="utf-8")),
        "oracle_sha256": sha256(FIXTURE / "oracle.json"),
        "first_build": first,
        "second_build": second,
        "byte_identical": True,
    }
    (args.out_dir / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
