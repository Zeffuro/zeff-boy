#!/usr/bin/env python3
"""Verify descriptor structure, native APU writes, and replay PCM for the open fixture."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import wave
import zipfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests/fixtures/huge"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def run(exe, args, log):
    result = subprocess.run([str(exe), *map(str,args)], cwd=ROOT, text=True,
                            capture_output=True, timeout=120,
                            env={**os.environ, "ZEFF_MUTE_AUDIO":"1", "CARGO_INCREMENTAL":"0"},
                            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    log.write_text(result.stdout+result.stderr, encoding="utf-8")
    if result.returncode:
        raise RuntimeError(f"command failed; see {log}")
    return result.stdout


def expected_writes(oracle, frames=260):
    writes = [(0xff26,0), (0xff26,0x80), (0xff25,0xff), (0xff24,0x77)]
    ticks = []
    wave_loaded = False
    for tick in range(frames):
        order, row = divmod(tick % oracle["loop_ticks"], 64)
        if row not in oracle["note_rows"]:
            continue
        ticks.append(tick)
        index = oracle["note_rows"].index(row)
        periods = [channel["periods"][order][index] for channel in oracle["channels"][:3]]
        noise = oracle["channels"][3]["polynomials"][order][index]
        writes += [(0xff10,0), (0xff11,0x80), (0xff12,0xf0),
                   (0xff13,periods[0]&255), (0xff14,0x80|(periods[0]>>8)),
                   (0xff16,0x80), (0xff17,0xf0),
                   (0xff18,periods[1]&255), (0xff19,0x80|(periods[1]>>8)),
                   (0xff1b,0), (0xff1c,0x20)]
        if not wave_loaded:
            writes += [(0xff25,0xbb), (0xff1a,0)]
            writes += [(0xff30+index,value) for index,value in enumerate(oracle["wave"])]
            writes += [(0xff1a,0x80), (0xff25,0xff)]
            wave_loaded = True
        writes += [(0xff25,0xbb), (0xff1a,0), (0xff1a,0xff),
                   (0xff1d,periods[2]&255), (0xff1e,0x80|(periods[2]>>8)), (0xff25,0xff),
                   (0xff21,0xf0), (0xff20,0), (0xff22,noise), (0xff23,0x80)]
    return writes, ticks


def verify_structure(report, source, receipt, oracle):
    build = receipt["first_build"]
    symbols = {name:item["address"] for name,item in build["symbols"].items()}
    assert report["source_sha256"] == digest(source)
    assert report["format_reference_revision"] == receipt["upstream"]["revision"]
    assert report["binding"] == "explicit_descriptor"
    song = report["song"]
    assert (song["order_count"],song["ticks_per_row"],song["loop_ticks"]) == (2,1,128)
    expected = [(order*64+row,channel,oracle["channels"][channel]["notes"][order][index],1,True)
                for order in range(2) for index,row in enumerate(oracle["note_rows"]) for channel in range(4)]
    assert [(n["tick"],n["channel"],n["pitch"],n["instrument"],n["reload_instrument"]) for n in song["notes"]] == expected
    expected_spans = [(symbols["FixtureSong"],21), (symbols["FixtureOrderCount"],1)]
    expected_spans += [(symbols[f"FixtureOrder{channel}"],4) for channel in range(1,5)]
    patterns = ["Pulse1A","Pulse1B","Pulse2A","Pulse2B","WaveA","WaveB","NoiseA","NoiseB"]
    expected_spans += [(symbols[name],192) for name in patterns]
    expected_spans += [(symbols[name],6) for name in ["FixtureDuty","FixtureWaveInstrument","FixtureNoise"]]
    expected_spans += [(symbols["FixtureWaves"],16)]
    assert [(s["offset"],s["byte_len"]) for s in song["spans"]] == sorted(expected_spans)
    assert len(song["instruments"]) == 4
    for instrument in song["instruments"]:
        at = instrument["source"]["offset"]
        assert instrument["data"] == list(source[at:at+6])
    return [list(note) for note in expected]


def verify(exe, inspector, fixture, out):
    receipt = json.loads((fixture/"receipt.json").read_text())
    for name, expected in receipt["fixture_hashes"].items():
        assert digest((FIXTURE/name).read_bytes()) == expected
    assert receipt["byte_identical"] and receipt["first_build"] == receipt["second_build"]
    build = receipt["first_build"]
    rom = fixture/"build-1"/build["rom"]
    source = rom.read_bytes()
    assert digest(source) == build["sha256"]
    oracle = json.loads((FIXTURE/"oracle.json").read_text())
    report = json.loads(run(inspector,[rom,build["symbols"]["FixtureSong"]["address"]],out/"structure.log"))
    normalized = verify_structure(report,source,receipt,oracle)
    (out/"structure.json").write_text(json.dumps(report,indent=2)+"\n")
    automatic = json.loads(run(inspector,[rom],out/"discovery.log"))
    assert automatic["binding"] == "static_literal_init"
    assert automatic["source_sha256"] == digest(source)
    assert automatic["format_reference_revision"] == receipt["upstream"]["revision"]
    discovery = automatic["discovery"]
    assert len(discovery["bound"]) == 1 and discovery["held"] == []
    bound = discovery["bound"][0]
    assert bound["song"] == report["song"]
    symbols = {name:item["address"] for name,item in build["symbols"].items()}
    evidence = bound["evidence"]
    assert evidence["init_address"] == symbols["hUGE_init"]
    assert evidence["update_address"] == symbols["hUGE_dosound"]
    assert evidence["ram_address"] == receipt["layout"]["ram"]
    assert evidence["driver"] == {"offset":symbols["hUGE_init"],"byte_len":1941}
    assert len(bound["init_calls"]) == 1
    assert bound["init_calls"][0]["instruction"] == {"offset":symbols["FixtureInitCall"],"byte_len":3}
    assert bound["init_calls"][0]["roots"] & 1
    assert len(evidence["irq_update_calls"]) == 1
    assert evidence["irq_update_calls"][0]["instruction"] == {"offset":symbols["FixtureUpdateCall"],"byte_len":3}
    assert evidence["irq_update_calls"][0]["roots"] & 2
    (out/"discovery.json").write_text(json.dumps(automatic,indent=2)+"\n")
    result = verify_native(exe,rom,out,oracle)
    result.update({"inspector_sha256":digest(inspector.read_bytes()),"normalized_notes":normalized,
                   "automatic_binding":True,"driver_address":evidence["init_address"],
                   "scope":"Static executable/descriptor binding and reset-run fixture replay; no generalized native rip qualification."})
    return result


def verify_native(exe, rom, out, oracle):
    source = rom.read_bytes()
    capture, dump = out/"native.zip", out/"native.f32"
    log = run(exe,["--headless","--no-sram","--max-frames","260",rom,
                   "--audio-trace",capture,"--audio-dump",dump],out/"native.log")
    rate = int(re.search(r"sample_rate=(\d+)",log).group(1))
    with zipfile.ZipFile(capture) as archive:
        trace = json.loads(archive.read("trace.json"))
    assert trace["dropped_events"] == 0 and trace["invalidated"] is None
    actual, timed, starts = [], [], []
    for event in trace["events"]:
        write = event["write"].get("register",event["write"].get("wave_ram"))
        if write is not None:
            assert write["origin"] == "cpu"
            actual.append((write["address"],write["value"]))
            timed.append((event["cycle"],write["address"],write["value"]))
            if write["address"] == 0xff10:
                starts.append(event["cycle"])
    expected,ticks = expected_writes(oracle)
    assert actual == expected, "native register sequence differs from the authored oracle"
    assert len(starts) == len(ticks)
    assert [cycle-starts[0] for cycle in starts] == [tick*70224 for tick in ticks]
    validation, wav = out/"replay.json", out/"music.wav"
    run(exe,["--audio-capture-check",validation,capture,"--audio-sample-rate",rate,
             "--audio-max-seconds","6",
             "--audio-capture-reference-f32",dump,"--audio-capture-wav",wav],out/"replay.log")
    replay = json.loads(validation.read_text())["outcome"]
    assert replay["native_reference"]["status"] == "matched"
    assert replay["wav_export"]["status"] == "written"
    with wave.open(str(wav),"rb") as audio:
        assert (audio.getnchannels(),audio.getsampwidth(),audio.getframerate(),audio.getcomptype()) == (2,2,rate,"NONE")
        pcm = audio.readframes(audio.getnframes())
        assert audio.getnframes() > 0 and any(pcm)
        pcm_frames = audio.getnframes()
    assert rom.read_bytes() == source
    return {"passed":True,"source_sha256":digest(source),"application_sha256":digest(exe.read_bytes()),
            "apu_writes":len(actual),"note_ticks":ticks,
            "timed_writes_sha256":digest(json.dumps(timed).encode()),
            "native_f32_sha256":digest(dump.read_bytes()),"pcm_sha256":digest(pcm),
            "pcm_frames":pcm_frames,"sample_rate":rate}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--zeff-boy",type=Path,required=True)
    parser.add_argument("--inspector",type=Path,required=True)
    parser.add_argument("--fixture-dir",type=Path,required=True)
    parser.add_argument("--out-dir",type=Path,required=True)
    parser.add_argument("--compare-proof",type=Path,help="require normalized output and PCM equality to another layout")
    args = parser.parse_args()
    out = args.out_dir.resolve()
    out.mkdir(parents=True,exist_ok=False)
    result = verify(args.zeff_boy.resolve(),args.inspector.resolve(),args.fixture_dir.resolve(),out)
    if args.compare_proof:
        other = json.loads(args.compare_proof.read_text())
        assert other["passed"]
        for field in ["normalized_notes","timed_writes_sha256","native_f32_sha256","pcm_sha256","pcm_frames","sample_rate"]:
            assert result[field] == other[field], f"layout mismatch: {field}"
        result["compared_proof_sha256"] = digest(args.compare_proof.read_bytes())
    (out/"proof.json").write_text(json.dumps(result,indent=2)+"\n")
    print(json.dumps(result,indent=2))


if __name__ == "__main__":
    main()
