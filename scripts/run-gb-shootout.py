#!/usr/bin/env python3

import argparse
import base64
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


FPS = 59.7275
SCREEN_SIZE = (160, 144)
SGB_SCREEN_SIZE = (256, 224)


def parse_args():
    parser = argparse.ArgumentParser(
        description="Run Zeff Boy against a local GBEmulatorShootout checkout."
    )
    parser.add_argument("--shootout-dir", type=Path, required=True)
    parser.add_argument(
        "--executable", type=Path, default=Path("target/release/zeff-boy.exe")
    )
    parser.add_argument("--test", action="append", default=[])
    parser.add_argument("--model", action="append", choices=("DMG", "CGB", "SGB"))
    parser.add_argument("--max-tests", type=int)
    parser.add_argument("--screenshot-every", type=int, default=6)
    parser.add_argument("--extra-seconds", type=float, default=5.0)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def load_tests(shootout_dir):
    sys.path.insert(0, str(shootout_dir))
    try:
        import testroms.acid
        import testroms.ashiepaws
        import testroms.ax6
        import testroms.blargg
        import testroms.cpp
        import testroms.daid
        import testroms.mealybug
        import testroms.mooneye
        import testroms.samesuite
    except ModuleNotFoundError as error:
        raise SystemExit(
            f"Missing {error.name}; install the shootout requirements with "
            f"'{sys.executable} -m pip install -r "
            f"{shootout_dir / 'requirements-core.txt'}'."
        ) from error

    return (
        testroms.acid.all
        + testroms.blargg.all
        + testroms.daid.all
        + testroms.ax6.all
        + testroms.mooneye.all
        + testroms.samesuite.all
        + testroms.ashiepaws.all
        + testroms.cpp.all
        + testroms.mealybug.all
    )


def selected(test, test_filters, models):
    return (not test_filters or any(value in str(test) for value in test_filters)) and (
        not models or test.model in models
    )


def normalized_screenshot(path, model):
    from PIL import Image

    with Image.open(path) as source:
        image = source.convert("RGBA")
    if model == "SGB" and image.size == SGB_SCREEN_SIZE:
        image = image.crop((48, 40, 208, 184))
    if image.size != SCREEN_SIZE:
        raise RuntimeError(f"unexpected {model} screenshot size {image.size}")
    return image


def encoded_png(image):
    import io

    output = io.BytesIO()
    image.save(output, format="PNG")
    return base64.b64encode(output.getvalue()).decode("ascii")


def run_test(executable, test, args):
    frame_limit = math.ceil((test.runtime + args.extra_seconds) * FPS)
    with tempfile.TemporaryDirectory(prefix="zeff-gb-shootout-") as temp_dir:
        command = [
            str(executable),
            "--headless",
            "--no-sram",
            "--mode",
            test.model.lower(),
            "--gb-dmg-palette",
            "gray",
            "--max-frames",
            str(frame_limit),
            "--screenshot-dir",
            temp_dir,
            "--screenshot-every",
            str(args.screenshot_every),
            str(Path(test.rom).resolve()),
        ]
        env = os.environ.copy()
        env["ZEFF_MUTE_AUDIO"] = "1"
        started = time.monotonic()
        process = subprocess.run(
            command,
            capture_output=True,
            encoding="utf-8",
            errors="replace",
            env=env,
            timeout=args.timeout,
        )
        elapsed = time.monotonic() - started
        if process.returncode != 0:
            detail = (process.stderr or process.stdout)[-2000:].strip()
            raise RuntimeError(f"Zeff Boy exited with {process.returncode}: {detail}")

        result = None
        screenshot = None
        for path in sorted(Path(temp_dir).glob("frame_*.png")):
            screenshot = normalized_screenshot(path, test.model)
            result = test.checkResult(screenshot)
            if result is not None:
                break
        if screenshot is None:
            raise RuntimeError("Zeff Boy produced no screenshots")
        return result or test.getDefaultResult(), screenshot, elapsed


def main():
    args = parse_args()
    shootout_dir = args.shootout_dir.resolve()
    executable = args.executable.resolve()
    output = args.output.resolve() if args.output else shootout_dir / "zeff_boy.json"
    if not (shootout_dir / "test.py").is_file():
        raise SystemExit(f"Not a GBEmulatorShootout checkout: {shootout_dir}")
    if not executable.is_file():
        raise SystemExit(f"Zeff Boy executable not found: {executable}")
    if args.screenshot_every < 1:
        raise SystemExit("--screenshot-every must be at least 1")

    os.chdir(shootout_dir)
    tests = [
        test
        for test in load_tests(shootout_dir)
        if selected(test, args.test, args.model)
    ]
    if args.max_tests is not None:
        tests = tests[: args.max_tests]
    if not tests:
        raise SystemExit("No shootout tests matched the filters")

    results = {}
    counts = {"PASS": 0, "FAIL": 0, "INFO": 0, "ERROR": 0}
    for index, test in enumerate(tests, 1):
        try:
            result, screenshot, runtime = run_test(executable, test, args)
            counts[result] = counts.get(result, 0) + 1
            results[str(test)] = {
                "result": result,
                "startuptime": 0.0,
                "runtime": runtime,
                "screenshot": encoded_png(screenshot),
            }
            print(f"[{index}/{len(tests)}] {result:4} {test}")
        except (RuntimeError, subprocess.TimeoutExpired) as error:
            counts["ERROR"] += 1
            print(f"[{index}/{len(tests)}] ERROR {test}: {error}", file=sys.stderr)

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(
            {
                "emulator": "Zeff Boy",
                "date": time.time(),
                "tests": results,
            },
            indent=2,
        ),
        encoding="utf-8",
    )
    print(f"Results: {counts}; wrote {output}")
    return 1 if counts["ERROR"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
