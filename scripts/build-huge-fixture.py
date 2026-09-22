#!/usr/bin/env python3
"""Build the pinned hUGEDriver fixture twice with an explicit RGBDS toolchain."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests/fixtures/huge"
URL = "https://github.com/SuperDisk/hUGEDriver.git"
REVISION = "a3cbd0cea48e6784d7f625066d0300f7cb075926"
INPUTS = ["hUGEDriver.asm", "include/hardware.inc", "include/hUGE.inc", "include/hUGE_note_table.inc", "README.md"]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args, cwd):
    result = subprocess.run(list(map(str, args)), cwd=cwd, text=True, capture_output=True,
                            check=False, timeout=120,
                            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    if result.returncode:
        raise RuntimeError(f"{args[0]} failed:\n{result.stdout}{result.stderr}")
    return result.stdout.strip()


def checkout(path):
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        run(["git", "clone", "--no-checkout", URL, path], ROOT)
        run(["git", "-C", path, "checkout", "--detach", REVISION], ROOT)
    if run(["git", "-C", path, "rev-parse", "HEAD"], ROOT) != REVISION:
        raise ValueError("existing upstream checkout differs from the pin; use a new work directory")
    if run(["git", "-C", path, "status", "--porcelain", "--untracked-files=all"], ROOT):
        raise ValueError("upstream checkout is dirty")
    return path


def build(out, source, tools, code, song, ram):
    out.mkdir()
    objects = []
    for name, path in [("driver", source/"hUGEDriver.asm"), ("player", FIXTURE/"player.asm"), ("song", FIXTURE/"song.asm")]:
        obj = out/(name+".o")
        run([tools["rgbasm"], "-I", str(source)+os.sep, "-o", obj, path], out)
        objects.append(obj)
    script = out/"layout.link"
    script.write_text(f'ROM0\nORG ${code:x}\n"Sound Driver"\nORG ${song:x}\n"Fixture song"\nWRAM0\nORG ${ram:x}\n"Playback variables"\n')
    rom = out/"huge-four-channel.gb"
    sym = out/"fixture.sym"
    run([tools["rgblink"], "-l", script, "-m", out/"fixture.map", "-n", sym, "-o", rom, *objects], out)
    run([tools["rgbfix"], "-v", "-p", "0", "-m", "0", "-t", "HUGE FIXTURE", rom], out)
    if len(rom.read_bytes()) != 32768:
        raise ValueError("fixture is not a 32 KiB ROM-only image")
    symbols = {}
    for line in sym.read_text().splitlines():
        if line and not line.startswith(";"):
            address, name = line.split()
            if ":" not in address:
                continue
            bank, address = address.split(":")
            symbols[name] = {"bank":int(bank,16), "address":int(address,16)}
    return {"rom":rom.name, "sha256":digest(rom), "symbols":symbols}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rgbds", type=Path, required=True, help="directory containing RGBDS executables")
    parser.add_argument("--source", type=Path, default=ROOT/".tmp/huge-fixture-source")
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--code", type=lambda v:int(v,0), default=0x200)
    parser.add_argument("--song", type=lambda v:int(v,0), default=0x2000)
    parser.add_argument("--ram", type=lambda v:int(v,0), default=0xc000)
    args = parser.parse_args()
    out = args.out_dir.resolve()
    if out.exists():
        raise ValueError("output directory exists")
    if not 0x200 <= args.code < args.song < 0x3800 or not 0xc000 <= args.ram <= 0xcf00:
        raise ValueError("invalid fixture layout")
    tools = {name:(args.rgbds/(name+(".exe" if os.name == "nt" else ""))).resolve() for name in ["rgbasm","rgblink","rgbfix"]}
    versions = {name:run([path,"--version"],ROOT) for name,path in tools.items()}
    if any("v1.0.3" not in version for version in versions.values()):
        raise ValueError("this fixture requires RGBDS v1.0.3")
    source = checkout(args.source.resolve())
    out.mkdir(parents=True)
    first = build(out/"build-1",source,tools,args.code,args.song,args.ram)
    second = build(out/"build-2",source,tools,args.code,args.song,args.ram)
    if first != second:
        raise ValueError("independent builds differ")
    receipt = {"upstream":{"url":URL,"revision":REVISION,"license":"public domain"},
               "tools":{name:{"path":str(path),"version":versions[name],"sha256":digest(path)} for name,path in tools.items()},
               "source_hashes":{name:digest(source/name) for name in INPUTS},
               "fixture_hashes":{name:digest(FIXTURE/name) for name in ["player.asm","song.asm","oracle.json"]},
               "layout":{"code":args.code,"song":args.song,"ram":args.ram},
               "first_build":first,"second_build":second,"byte_identical":True}
    (out/"receipt.json").write_text(json.dumps(receipt,indent=2)+"\n")
    print(json.dumps({"passed":True,"sha256":first["sha256"],"output":str(out)}))


if __name__ == "__main__":
    main()
