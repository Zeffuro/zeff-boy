#!/usr/bin/env python3
"""Verify automatic table exports, including the song the fixture never starts."""

import argparse
import importlib.util
import json
from pathlib import Path
import wave
import zipfile


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("psglib_verify", Path(__file__).with_name("verify-psglib-fixture.py"))
common = importlib.util.module_from_spec(spec)
spec.loader.exec_module(common)


def verify(exe, fixture, out):
    receipt = json.loads((fixture / "receipt.json").read_text())
    selector = receipt["table_selector"]
    assert selector in (0, 1) and receipt["byte_identical"]
    sources = ROOT / "tests/fixtures/psglib"
    for name, expected in receipt["fixture_sources"].items():
        assert common.digest((sources / name).read_bytes()) == expected
    assert common.digest((sources / "oracle.json").read_bytes()) == receipt["oracle_sha256"]
    assert json.loads((sources / "oracle.json").read_text()) == receipt["oracle"]
    build = receipt["first_build"]
    symbols = build["symbols"]
    rom = fixture / "build-1" / build["rom"]
    source = rom.read_bytes()
    assert common.digest(source) == build["sha256"] == receipt["second_build"]["sha256"]
    oracle = receipt["oracle"]["songs"]
    offsets = [symbols[song["symbol"]] for song in oracle]
    table = symbols["song_table"]
    wrapper = symbols["table_play"]
    expected = bytes.fromhex("fe02d06f26002911") + table.to_bytes(2, "little")
    expected += bytes.fromhex("195e2356ebc3") + symbols["PSGPlay"].to_bytes(2, "little")
    assert source[wrapper:wrapper + 18] == expected
    assert source[table:table + 4] == b"".join(at.to_bytes(2, "little") for at in offsets)
    assert len(build["table_call_sites"]) == 1

    capture = out / "native.zip"
    common.run(exe, ["--headless", "--no-sram", "--max-frames", "30", rom,
                     "--audio-trace", capture], out / "native.log")
    with zipfile.ZipFile(capture) as archive:
        trace = json.loads(archive.read("trace.json"))
    assert trace["invalidated"] is None and trace["dropped_events"] == 0
    assert trace["start"] == "reset" and trace["timing"] == "instruction_boundary"
    assert trace["cycle_hz"] == 3_584_160 and trace.get("cycle_hz_denominator", 1) == 1
    native = {}
    for event in trace["events"]:
        write = event["write"]["sn76489"]
        assert write["port"] == 0x7f
        native.setdefault(event["cycle"] // common.FIXTURE_CYCLES_PER_FRAME, []).append(write["value"])
    first = min(native)
    expected = oracle[selector]["frames"]
    actual = [native.get(frame, []) for frame in range(first, first + len(expected))]
    assert actual == expected, (actual, expected)
    assert sum(map(len, expected)) == len(trace["events"])

    rows = []
    for rate in (50, 60):
        bundle = out / f"streams-{rate}.zip"
        common.run(exe, ["--audio-psglib", bundle, rom, "--psglib-rate", rate,
                         "--psglib-auto"], out / f"export-{rate}.log")
        with zipfile.ZipFile(bundle) as archive:
            manifest = json.loads(archive.read("manifest.json"))
            assert manifest["source"]["sha256"] == common.digest(source)
            report = manifest["selection"]["discovery"]
            assert not report["held"] and report["candidate_count"] == 2
            assert [song["offset"] for song in report["bound"]] == sorted(offsets)
            assert len(manifest["streams"]) == 2
            by_offset = {song["offset"]: song for song in report["bound"]}
            for index, (offset, song) in enumerate(zip(offsets, oracle, strict=True)):
                bound = by_offset[offset]
                assert not bound["call_sites"] and not bound["call_roots"]
                evidence = bound["evidence"]
                assert evidence["psg_play"]["offset"] == symbols["PSGPlay"]
                assert evidence["psg_frame"]["offset"] == symbols["PSGFrame"]
                assert evidence["source_revision"] == receipt["upstream"]["commit"]
                assert evidence["frame_call_sites"] == [{"offset": at, "byte_len": 3} for at in build["psg_frame_call_sites"]]
                assert evidence["frame_call_roots"] == [2] * len(build["psg_frame_call_sites"])
                entries = bound["table_entries"]
                assert len(entries) == 1
                entry = entries[0]
                assert entry["selector"] == index and entry["count"] == 2
                assert entry["dispatcher"] == {"offset": wrapper, "byte_len": 18}
                assert entry["table"] == {"offset": table, "byte_len": 4}
                assert entry["entry"] == {"offset": table + 2 * index, "byte_len": 2}
                assert entry["call_sites"] == [{"offset": at, "byte_len": 3} for at in build["table_call_sites"]]
                assert entry["call_roots"] == [1]
                item = next(item for item in manifest["streams"] if item["offset"] == offset)
                events = json.loads(archive.read(item["events"]["path"]))
                static = [[] for _ in range(events["frames"])]
                for event in events["writes"]:
                    static[event["frame"]].append(event["value"])
                assert static == song["frames"]
                data = archive.read(item["vgm"]["path"])
                assert common.digest(data) == item["vgm"]["sha256"]
                ticks, writes = common.vgm_events(data)
                expected_writes = [[0, value] for value in [0x9f, 0xbf, 0xdf, 0xff]]
                expected_writes += [[frame * (44100 // rate), value]
                                    for frame, values in enumerate(static) for value in values]
                assert writes == expected_writes and ticks == len(static) * (44100 // rate)
                for retained in item["source_spans"]:
                    raw = archive.read(retained["path"])
                    start = retained["span"]["offset"]
                    assert raw == source[start:start + retained["span"]["byte_len"]]
                    assert common.digest(raw) == retained["sha256"]
                vgm = out / f"song-{index}-{rate}.vgm"
                vgm.write_bytes(data)
                wav = vgm.with_suffix(".wav")
                common.run(exe, ["--audio-discover", vgm.with_suffix(".json"), vgm,
                                 "--audio-export", "wav", wav, "--audio-song-offset", "0",
                                 "--audio-max-seconds", "1", "--audio-fade-seconds", "0",
                                 "--audio-sample-rate", "48000"], vgm.with_suffix(".log"))
                with wave.open(str(wav), "rb") as audio:
                    pcm = audio.readframes(audio.getnframes())
                    assert audio.getnframes() == len(static) * 48000 // rate and any(pcm)
                    rows.append({"selector": index, "rate": rate, "played_at_boot": index == selector,
                                 "frames": len(static), "vgm_sha256": common.digest(data),
                                 "pcm_sha256": common.digest(pcm), "pcm_frames": audio.getnframes()})
    assert source == rom.read_bytes()
    return {"passed": True, "source_sha256": common.digest(source),
            "application_sha256": common.digest(exe.read_bytes()), "boot_selector": selector,
            "native_event_count": len(trace["events"]), "native_frame_writes": actual,
            "discovered_streams": 2, "unplayed_streams_exported": 1, "projections": rows,
            "scope": "Finite table ABI; native register equivalence, chosen VGM/WAV timing; no retail coverage claim."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--zeff-boy", type=Path, required=True)
    parser.add_argument("--fixture-dir", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    out = args.out_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    proof = verify(args.zeff_boy.resolve(), args.fixture_dir.resolve(), out)
    (out / "proof.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(json.dumps(proof, indent=2))


if __name__ == "__main__":
    main()
