#!/usr/bin/env python3
"""Compare a built PSGlib fixture with static frame exports and native writes."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import wave
import zipfile


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_CYCLES_PER_FRAME = 342 * 262 * 2 // 3


def digest(data):
    return hashlib.sha256(data).hexdigest()


def run(exe, args, log):
    with log.open("x", encoding="utf-8") as output:
        subprocess.run(
            [str(exe), *map(str, args)], cwd=ROOT,
            env={**os.environ, "ZEFF_MUTE_AUDIO": "1", "CARGO_INCREMENTAL": "0"},
            stdout=output, stderr=subprocess.STDOUT, check=True, timeout=120,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )


def vgm_events(data):
    if data[:4] != b"Vgm " or int.from_bytes(data[4:8], "little") + 4 != len(data):
        raise ValueError("invalid projected VGM header")
    cursor = 0x34 + int.from_bytes(data[0x34:0x38], "little")
    ticks = 0
    writes = []
    while cursor < len(data):
        op = data[cursor]
        cursor += 1
        if op == 0x66:
            if cursor != len(data):
                raise ValueError("trailing VGM commands")
            return ticks, writes
        if op == 0x50:
            writes.append([ticks, data[cursor]])
            cursor += 1
        elif op == 0x61:
            ticks += int.from_bytes(data[cursor:cursor + 2], "little")
            cursor += 2
        else:
            raise ValueError(f"unexpected projected VGM command {op:#x}")
    raise ValueError("missing VGM terminator")


def verify_binding(selection, receipt, offsets, source):
    report = selection["discovery"]
    build = receipt["first_build"]
    layout = receipt.get("layout", {"code": 0x80, "data": 0xc000})
    code_delta, ram_delta = layout["code"] - 0x80, layout["data"] - 0xc000
    if (selection["kind"] != "static_calls" or selection["system"] != "sms"
            or selection["mapping"] != "reset_initial_identity_32k"
            or report["held"] or [song["offset"] for song in report["bound"]] != offsets):
        raise ValueError("automatic discovery did not bind exactly the fixture streams")
    wrapper = build["symbols"]["PSGPlayLoops"]
    play = int.from_bytes(source[wrapper + 1:wrapper + 3], "little")
    for song, expected_call in zip(report["bound"], build["psg_play_loops_call_sites"], strict=True):
        evidence = song["evidence"]
        if (evidence["variant"] != "devkitsms-psglib-f433a35d-sdcc-4.5.0"
                or evidence["source_revision"] != receipt["upstream"]["commit"]
                or evidence["psg_play"]["offset"] != play
                or evidence["psg_frame"] != {"offset": build["symbols"]["PSGFrame"], "byte_len": 0x177}
                or evidence["psg_play_loops"] != {"offset": wrapper, "byte_len": 22}
                or evidence["code_delta"] != code_delta or evidence["ram_delta"] != ram_delta
                or evidence["frame_call_sites"] != [{"offset": at, "byte_len": 3} for at in build["psg_frame_call_sites"]]
                or evidence["frame_call_roots"] != [2] * len(build["psg_frame_call_sites"])
                or song["call_roots"] != [1]
                or song["call_sites"] != [{"offset": expected_call - 3, "byte_len": 6}]):
            raise ValueError("automatic driver/call relocation evidence differs from link oracle")


def verify(exe, fixture, out, automatic=False):
    receipt = json.loads((fixture / "receipt.json").read_text(encoding="utf-8"))
    sources = ROOT / "tests" / "fixtures" / "psglib"
    for name, expected_hash in receipt["fixture_sources"].items():
        if digest((sources / name).read_bytes()) != expected_hash:
            raise ValueError("tracked fixture source differs from build receipt")
    oracle_bytes = (sources / "oracle.json").read_bytes()
    if digest(oracle_bytes) != receipt["oracle_sha256"] or json.loads(oracle_bytes) != receipt["oracle"]:
        raise ValueError("tracked oracle differs from build receipt")
    build = receipt["first_build"]
    rom = fixture / "build-1" / build["rom"]
    source = rom.read_bytes()
    if digest(source) != build["sha256"] or not receipt["byte_identical"]:
        raise ValueError("fixture build identity changed")
    oracle = receipt["oracle"]["songs"]
    offsets = [build["symbols"][song["symbol"]] for song in oracle]
    capture = out / "native.zip"
    run(exe, ["--headless", "--no-sram", "--max-frames", "30", rom,
              "--audio-trace", capture], out / "native.log")
    with zipfile.ZipFile(capture) as archive:
        trace = json.loads(archive.read("trace.json"))
    if (trace["invalidated"] is not None or trace["dropped_events"] != 0
            or trace["start"] != "reset" or trace["timing"] != "instruction_boundary"
            or trace["cycle_hz"] != 3_584_160 or trace.get("cycle_hz_denominator", 1) != 1):
        raise ValueError("unexpected or incomplete fixture trace contract")
    native = {}
    for event in trace["events"]:
        write = event["write"]["sn76489"]
        if write["port"] != 0x7f:
            raise ValueError("unexpected fixture sound port")
        native.setdefault(event["cycle"] // FIXTURE_CYCLES_PER_FRAME, []).append(write["value"])
    first = min(native)
    expected = [frame for song in oracle for frame in song["frames"]]
    actual = [native.get(frame, []) for frame in range(first, first + len(expected))]
    if actual != expected or sum(map(len, expected)) != len(trace["events"]):
        raise ValueError(f"native PSGFrame output differs: {actual!r}")
    rows = []
    for rate in [50, 60]:
        bundle = out / f"streams-{rate}.zip"
        args = ["--audio-psglib", bundle, rom, "--psglib-rate", rate]
        if automatic:
            args.append("--psglib-auto")
        else:
            for offset in offsets:
                args.extend(["--psglib-offset", offset])
        run(exe, args, out / f"export-{rate}.log")
        with zipfile.ZipFile(bundle) as archive:
            manifest = json.loads(archive.read("manifest.json"))
            if manifest["source"]["sha256"] != digest(source):
                raise ValueError("export source identity differs")
            if automatic:
                verify_binding(manifest["selection"], receipt, offsets, source)
            for offset, song, item in zip(offsets, oracle, manifest["streams"], strict=True):
                events = json.loads(archive.read(item["events"]["path"]))
                static = [[] for _ in range(events["frames"])]
                for event in events["writes"]:
                    static[event["frame"]].append(event["value"])
                if static != song["frames"]:
                    raise ValueError("static stream differs from original-driver oracle")
                data = archive.read(item["vgm"]["path"])
                if digest(data) != item["vgm"]["sha256"]:
                    raise ValueError("projected VGM identity differs")
                ticks, writes = vgm_events(data)
                ticks_per_frame = 44100 // rate
                expected_writes = [[0, value] for value in [0x9f, 0xbf, 0xdf, 0xff]]
                expected_writes += [[frame * ticks_per_frame, value]
                                    for frame, values in enumerate(static) for value in values]
                if writes != expected_writes or ticks != len(static) * ticks_per_frame:
                    raise ValueError("VGM write order or frame timing differs")
                for span in item["source_spans"]:
                    raw = archive.read(span["path"])
                    start = span["span"]["offset"]
                    if raw != source[start:start + span["span"]["byte_len"]] or digest(raw) != span["sha256"]:
                        raise ValueError("retained source bytes differ")
                vgm = out / f"stream-{offset:x}-{rate}.vgm"
                vgm.write_bytes(data)
                wav = vgm.with_suffix(".wav")
                run(exe, ["--audio-discover", vgm.with_suffix(".json"), vgm,
                          "--audio-export", "wav", wav, "--audio-song-offset", "0",
                          "--audio-max-seconds", "1", "--audio-fade-seconds", "0",
                          "--audio-sample-rate", "48000"], vgm.with_suffix(".log"))
                with wave.open(str(wav), "rb") as audio:
                    pcm = audio.readframes(audio.getnframes())
                    if audio.getnframes() != len(static) * 48000 // rate or not any(pcm):
                        raise ValueError("projected WAV has wrong duration or is silent")
                    rows.append({"offset": offset, "rate": rate, "frames": len(static),
                                 "writes": len(events["writes"]), "vgm_sha256": digest(data),
                                 "pcm_sha256": digest(pcm), "pcm_frames": audio.getnframes()})
    if rom.read_bytes() != source:
        raise ValueError("fixture source was modified")
    return {"schema": "zeff-psglib-fixture-proof/1", "passed": True,
            "automatic_discovery": automatic,
            "source_sha256": digest(source), "application_sha256": digest(exe.read_bytes()),
            "native_event_count": len(trace["events"]), "native_frame_writes": actual,
            "projection_results": rows,
            "scope": "Per-frame register equivalence; projected VGM/WAV timing is chosen, not native PCM equivalence."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--zeff-boy", type=Path, required=True)
    parser.add_argument("--fixture-dir", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--automatic", action="store_true", help="discover stream offsets from driver calls")
    args = parser.parse_args()
    out = args.out_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    result = verify(args.zeff_boy.resolve(), args.fixture_dir.resolve(), out, args.automatic)
    (out / "proof.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
