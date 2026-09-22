#!/usr/bin/env python3
"""Compare the open hUGE fixture with copied-audio-only playback images."""

import argparse
import json
import math
from pathlib import Path
import runpy

ROOT = Path(__file__).resolve().parents[1]
HELPERS = runpy.run_path(str(ROOT/"scripts/verify-huge-fixture.py"))
run, digest = HELPERS["run"], HELPERS["digest"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--zeff-boy",type=Path,required=True)
    parser.add_argument("--isolator",type=Path,required=True)
    parser.add_argument("--closure",type=Path,required=True)
    parser.add_argument("--fixture-dir",type=Path,required=True)
    parser.add_argument("--original-proof",type=Path,required=True)
    parser.add_argument("--out-dir",type=Path,required=True)
    args = parser.parse_args()
    out = args.out_dir.resolve()
    out.mkdir(parents=True,exist_ok=False)
    receipt = json.loads((args.fixture_dir/"receipt.json").read_text())
    assert receipt["byte_identical"] and receipt["first_build"] == receipt["second_build"]
    for name, expected in receipt["fixture_hashes"].items():
        assert digest((ROOT/"tests/fixtures/huge"/name).read_bytes()) == expected
    rom = (args.fixture_dir/"build-1"/receipt["first_build"]["rom"]).resolve()
    assert digest(rom.read_bytes()) == receipt["first_build"]["sha256"]
    original = json.loads(args.original_proof.read_text())
    assert original["passed"] and original["source_sha256"] == digest(rom.read_bytes())
    descriptor = receipt["first_build"]["symbols"]["FixtureSong"]["address"]
    oracle = json.loads((ROOT/"tests/fixtures/huge/oracle.json").read_text())
    frames = oracle["loop_ticks"] + 2 * math.lcm(oracle["loop_ticks"],256) + 1
    closure = json.loads(run(args.closure.resolve(),[rom,descriptor,frames],out/"closure.log"))
    assert closure["passed"] and closure["source_sha256"] == digest(rom.read_bytes())
    assert closure["descriptor"] == descriptor and closure["frames"] == frames
    assert closure["original"]["update_count"] == frames
    assert closure["original"]["sound_writes"] == len(HELPERS["expected_writes"](oracle,frames)[0])
    assert closure["original"]["recurrence"]["passed"]
    (out/"closure.json").write_text(json.dumps(closure,indent=2)+"\n")
    layouts = []
    for index,fill in enumerate([0,255]):
        directory = out/f"fill-{fill}"
        directory.mkdir()
        isolated_rom = directory/"isolated.gb"
        isolated = json.loads(run(args.isolator.resolve(),[rom,descriptor,fill,isolated_rom],directory/"isolation.log"))
        assert closure["isolated"][index]["fill"] == fill
        assert isolated["source_sha256"] == digest(rom.read_bytes())
        assert isolated["isolated_sha256"] == digest(isolated_rom.read_bytes()) == closure["isolated"][index]["rom_sha256"]
        result = HELPERS["verify_native"](args.zeff_boy.resolve(),isolated_rom,directory,oracle)
        for field in ["apu_writes","note_ticks","timed_writes_sha256","native_f32_sha256","pcm_sha256","pcm_frames","sample_rate"]:
            assert result[field] == original[field], f"isolated output differs: {field}"
        (directory/"proof.json").write_text(json.dumps(result,indent=2)+"\n")
        layouts.append({"fill":fill,"isolation":isolated,"native":result})
    result = {"passed":True,"source_sha256":digest(rom.read_bytes()),"descriptor":descriptor,
              "closure_sha256":digest((out/"closure.json").read_bytes()),
              "original_proof_sha256":digest(args.original_proof.read_bytes()),
              "closure_executable_sha256":digest(args.closure.read_bytes()),
              "isolator_executable_sha256":digest(args.isolator.read_bytes()),
              "layouts":layouts,
              "scope":"Bounded CPU-access closure, identical original/isolated native sound writes and PCM for the authored DMG fixture; no generalized export admission."}
    (out/"proof.json").write_text(json.dumps(result,indent=2)+"\n")
    print(json.dumps({"passed":True,"descriptor":descriptor,"layouts":2,"sound_writes":213,
                      "pcm_frames":original["pcm_frames"],"pcm_sha256":original["pcm_sha256"]}))


if __name__ == "__main__":
    main()
