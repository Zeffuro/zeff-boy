#!/usr/bin/env python3
"""Build, train, and verify a native PGO Zeff Boy artifact."""
import argparse, hashlib, inspect, json, os, re, shutil, subprocess, sys, tomllib
from pathlib import Path

CORES = {
    "GB": "zeff_gb_core",
    "GBC": "zeff_gb_core",
    "GBA": "zeff_gba_core",
    "NES": "zeff_nes_core",
    "PCE": "zeff_pce_core",
    "SMS": "zeff_sega8_core",
    "GG": "zeff_sega8_core",
    "WS": "zeff_ws_core",
    "WSC": "zeff_ws_core",
}
REQUIRED_CORES = set(CORES.values())
ANSI_ESCAPE = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
CARGO_WARNING_SUMMARY = re.compile(
    r'^warning: `[A-Za-z0-9_-]+` '
    r'\((?:lib(?: test)?|(?:bin|example|test|bench) "[^"\r\n]+"(?: test)?|build script)\) '
    r'generated (?:1 warning|[2-9]\d* warnings|1\d+ warnings)'
    r'(?: \((?:1 duplicate|[2-9]\d* duplicates|1\d+ duplicates)\))?$'
)
MISSING_FUNCTION = re.compile(
    r"^warning:\s*(?:\S+:\s+)?"
    r"no profile data available for function\s+\S+\s+"
    r"Hash\s*=\s*\d+\s+up to\s+0\s+count discarded\s*$",
    re.I,
)
PROFILE_DIAGNOSTIC = re.compile(
    r"(?:warning|error):.*(?:profile|pgo)|"
    r"function (?:control flow|basic block count|value site count|bitmap size) "
    r"change detected|inconsistent number of counts|"
    r"(?:hash|counter|bitmap size) mismatch|no profile data available",
    re.I,
)
COMMAND_TIMEOUT = 3600


def profile_generate_flags():
    return "-Cprofile-generate=."


def strip_c_pgo_args(arguments):
    result = []
    for argument in arguments:
        if argument in ("-fprofile-generate", "-fprofile-use"):
            continue
        if argument.startswith("-fprofile-generate=") or argument.startswith(
            "-fprofile-use="
        ):
            continue
        result.append(argument)
    return result


def macos_compiler_args(arguments, sdk_path):
    result = strip_c_pgo_args(arguments)
    has_sysroot = any(
        argument == "-isysroot" or argument.startswith("-isysroot=")
        for argument in result
    )
    if not has_sysroot:
        result = ["-isysroot", sdk_path, *result]
    return result


def preflight_macos_wrapper(wrapper, language, repository, environment):
    source = "#include <TargetConditionals.h>\nint main(void) { return 0; }\n"
    command = [str(wrapper), "-x", language, "-fsyntax-only", "-"]
    result = subprocess.run(
        command,
        cwd=repository,
        env=environment,
        input=source,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=60,
    )
    if result.returncode:
        raise RuntimeError(
            f"macOS {language} wrapper cannot compile SDK headers:\n{result.stdout}"
        )


def configure_macos_c_wrappers(output_root, host, environment, repository):
    if "apple-darwin" not in host:
        return None
    normalized = host.replace("-", "_")
    names = (
        "CC",
        "CXX",
        f"CC_{host}",
        f"CXX_{host}",
        f"CC_{normalized}",
        f"CXX_{normalized}",
        "TARGET_CC",
        "TARGET_CXX",
    )
    conflicts = {key for key in names if environment.get(key)}
    if conflicts:
        raise RuntimeError(
            "macOS compiler overrides must be unset: " + ",".join(sorted(conflicts))
        )
    wrapper_root = output_root / "compiler-wrappers"
    wrapper_root.mkdir()
    sdk_path = output(["xcrun", "--sdk", "macosx", "--show-sdk-path"], repository)
    if not Path(sdk_path).is_dir():
        raise RuntimeError(f"xcrun returned a missing macOS SDK: {sdk_path}")
    if not environment.get("MACOSX_DEPLOYMENT_TARGET"):
        environment["MACOSX_DEPLOYMENT_TARGET"] = (
            "11.0" if host.startswith("aarch64-") else "10.12"
        )
    details = {}
    for language, compiler_name in (("c", "clang"), ("cxx", "clang++")):
        compiler = output(["xcrun", "--find", compiler_name], repository)
        identity = output([compiler, "--version"], repository)
        # Keep the compiler basename so cc-rs recognizes the wrapper as Clang.
        wrapper = wrapper_root / compiler_name
        source = (
            "#!/usr/bin/env python3\nimport os,sys\n\n"
            + inspect.getsource(strip_c_pgo_args)
            + "\n"
            + inspect.getsource(macos_compiler_args)
            + f"\ncompiler={compiler!r}\n"
            + f"sdk_path={sdk_path!r}\n"
            + "os.execv(compiler,[compiler,*macos_compiler_args(sys.argv[1:],sdk_path)])\n"
        )
        wrapper.write_text(source, encoding="utf-8")
        wrapper.chmod(0o755)
        details[language] = {
            "compiler": compiler,
            "identity": identity,
            "sdk_path": sdk_path,
            "wrapper": str(wrapper),
            "wrapper_sha256": digest(wrapper),
        }
    environment["CC"] = details["c"]["wrapper"]
    environment["CXX"] = details["cxx"]["wrapper"]
    preflight_macos_wrapper(details["c"]["wrapper"], "c", repository, environment)
    preflight_macos_wrapper(
        details["cxx"]["wrapper"], "c++", repository, environment
    )
    return details


def reject_native_config_rustflags(root, host, environment):
    cargo_home = Path(environment.get("CARGO_HOME", Path.home() / ".cargo"))
    configs = (
        root / ".cargo" / "config.toml",
        root / ".cargo" / "config",
        cargo_home / "config.toml",
        cargo_home / "config",
    )
    for config in configs:
        if not config.is_file():
            continue
        data = tomllib.loads(config.read_text(encoding="utf-8"))
        if data.get("build", {}).get("rustflags"):
            raise RuntimeError(f"global Cargo rustflags are unsupported: {config}")
        for target, values in data.get("target", {}).items():
            if not isinstance(values, dict) or not values.get("rustflags"):
                continue
            if target.startswith("wasm32-") and "wasm32" not in host:
                continue
            raise RuntimeError(
                f"native-affecting Cargo target rustflags are unsupported: {config} [{target}]"
            )


def digest(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        for block in iter(lambda: f.read(1048576), b""):
            h.update(block)
    return h.hexdigest()


def fingerprint(root):
    paths = [
        root / n
        for n in (
            "Cargo.toml",
            "Cargo.lock",
            "build.rs",
            "rust-toolchain",
            "rust-toolchain.toml",
        )
        if (root / n).is_file()
    ]
    for n in (".cargo", "assets", "src", "crates", "third_party"):
        if (root / n).is_dir():
            paths += [p for p in (root / n).rglob("*") if p.is_file()]
    rows = [
        f"{p.relative_to(root).as_posix()}\t{digest(p)}" for p in sorted(set(paths))
    ]
    return {
        "sha256": hashlib.sha256("\n".join(rows).encode()).hexdigest(),
        "files": len(rows),
    }


def classify_profile_diagnostics(text):
    """Separate LLVM unknown-function records from invalid/stale profile data.

    LLVM appends Hash/count-discarded to both missing and mismatched records.
    A missing record is not proof that the function was merely unexecuted.
    """
    missing, rejected = [], []
    for line in ANSI_ESCAPE.sub("", text).splitlines():
        if CARGO_WARNING_SUMMARY.fullmatch(line.strip()):
            continue
        if MISSING_FUNCTION.fullmatch(line.strip()):
            missing.append(line)
        elif PROFILE_DIAGNOSTIC.search(line):
            rejected.append(line)
    return {"missing_function_count": len(missing), "rejected_count": len(rejected)}


def execute(cmd, cwd, env, log, reject=False, quiet=False, diagnostics=None):
    result = subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=COMMAND_TIMEOUT,
    )
    Path(log).write_text(
        "> " + subprocess.list2cmdline(cmd) + "\n" + result.stdout, encoding="utf-8"
    )
    if not quiet:
        sys.stdout.write(result.stdout)
    if result.returncode:
        raise RuntimeError(f"{cmd[0]} exited {result.returncode}; see {log}")
    if reject:
        classified = classify_profile_diagnostics(result.stdout)
        if diagnostics is not None:
            diagnostics.update(classified)
        if classified["rejected_count"]:
            raise RuntimeError(f"invalid or unexpected profile diagnostics found; see {log}")
    return result.stdout


def output(cmd, root):
    return subprocess.check_output(cmd, cwd=root, text=True, encoding="utf-8").strip()


def tools(root):
    rustc = output(["rustc", "-vV"], root)
    host = re.search(r"^host:\s*(.+)$", rustc, re.M)
    llvm = re.search(r"^LLVM version:\s*(\d+)", rustc, re.M)
    if not host or not llvm:
        raise RuntimeError("rustc host/LLVM metadata missing")
    suffix = ".exe" if os.name == "nt" else ""
    prof = (
        Path(output(["rustc", "--print", "sysroot"], root))
        / "lib"
        / "rustlib"
        / host.group(1)
        / "bin"
        / f"llvm-profdata{suffix}"
    )
    if not prof.is_file():
        raise RuntimeError("install matching llvm-tools-preview")
    version = output([str(prof), "--version"], root)
    major = re.search(r"(?:LLVM )?version\s+(\d+)", version, re.I)
    if not major or major.group(1) != llvm.group(1):
        raise RuntimeError("llvm-profdata version mismatch")
    return {
        "host": host.group(1),
        "rustc": rustc,
        "cargo": output(["cargo", "--version"], root),
        "profdata": str(prof),
        "profdata_version": version,
        "profdata_sha256": digest(prof),
    }


def corpus(path):
    path = Path(path).resolve()
    data = json.loads(path.read_text(encoding="utf-8"))
    if data.get("schema") != 1 or not isinstance(data.get("fixtures"), list):
        raise RuntimeError("invalid corpus schema")
    result = []
    ids = set()
    for item in data["fixtures"]:
        if not {"id", "system", "path", "sha256", "frames"} <= item.keys():
            raise RuntimeError("invalid corpus fixture")
        fixture_id = str(item["id"])
        system = str(item["system"]).upper()
        rel = Path(str(item["path"]))
        if (
            not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", fixture_id)
            or fixture_id in ids
        ):
            raise RuntimeError("fixture ids must be unique safe filenames")
        ids.add(fixture_id)
        if system not in CORES or rel.is_absolute() or ".." in rel.parts:
            raise RuntimeError("invalid corpus fixture path/system")
        rom = (path.parent / rel).resolve()
        if path.parent not in rom.parents:
            raise RuntimeError("fixture resolves outside corpus directory")
        if not rom.is_file() or digest(rom) != str(item["sha256"]).lower():
            raise RuntimeError(f"fixture hash mismatch: {item['id']}")
        frames = int(item["frames"])
        if frames < 1:
            raise RuntimeError("fixture frames must be positive")
        result.append(
            {
                **item,
                "system": system,
                "path": rom,
                "frames": frames,
                "core_crate": CORES[system],
            }
        )
    if not result:
        raise RuntimeError("empty corpus")
    return result


def names(kind, host):
    exe = ".exe" if "windows" in host else ""
    lib = (
        "zeff_libretro.dll"
        if exe
        else ("libzeff_libretro.dylib" if "apple" in host else "libzeff_libretro.so")
    )
    return (lib if kind == "libretro" else "zeff-boy" + exe, "libretro_harness" + exe)


def semantics(text):
    ignored = {"fps_p50", "fps_p95", "elapsed_ms_p50", "elapsed_ms_p95"}
    lines = [line for line in text.splitlines() if line.startswith("runs=")]
    if len(lines) != 1:
        raise RuntimeError("harness did not emit exactly one summary")
    result = {
        k: v
        for token in lines[0].split()
        if "=" in token
        for k, v in [token.split("=", 1)]
        if k not in ignored
    }
    required = {
        "callback_payload_hashing": "true",
        "state_roundtrip": "true",
        "repeated_state_hashes_match": "true",
        "repeated_video_hashes_match": "true",
        "repeated_audio_hashes_match": "true",
        "repeated_callback_counts_match": "true",
    }
    required_hashes = {
        "video_sha256",
        "audio_sha256",
        "serialize_sha256",
        "save_ram_sha256",
        "save_ram_post_roundtrip_sha256",
        "continuation_uninterrupted_video_sha256",
        "continuation_uninterrupted_audio_sha256",
        "continuation_uninterrupted_state_sha256",
        "continuation_restored_video_sha256",
        "continuation_restored_audio_sha256",
        "continuation_restored_state_sha256",
    }
    if any(result.get(key) != value for key, value in required.items()):
        raise RuntimeError("harness summary lacks required equality evidence")
    if not required_hashes <= result.keys():
        raise RuntimeError("harness summary lacks required hash evidence")
    return result


def main(argv=None):
    global COMMAND_TIMEOUT
    ap = argparse.ArgumentParser()
    ap.add_argument("--artifact", choices=("desktop", "libretro"), required=True)
    ap.add_argument("--output-dir", type=Path, required=True)
    ap.add_argument("--target-dir", type=Path)
    ap.add_argument("--corpus-manifest", type=Path)
    ap.add_argument("--extra-corpus-manifest", type=Path, action="append", default=[])
    ap.add_argument("--warmup", type=int, default=120)
    ap.add_argument("--verify-frames", type=int, default=120)
    ap.add_argument("--continuation-frames", type=int, default=30)
    ap.add_argument("--features", default="")
    ap.add_argument("--no-default-features", action="store_true")
    ap.add_argument("--offline", action="store_true")
    ap.add_argument(
        "--expected-target", help="Fail unless the native rustc host is this target"
    )
    ap.add_argument(
        "--timeout", type=int, default=3600, help="per-command timeout in seconds"
    )
    a = ap.parse_args(argv)
    COMMAND_TIMEOUT = a.timeout
    root = Path(__file__).resolve().parents[1]
    out = a.output_dir.resolve()
    if out.exists() or out == root or root in out.parents:
        raise RuntimeError("output directory must be fresh and outside repository")
    if min(a.verify_frames, a.continuation_frames, a.timeout) < 1 or a.warmup < 0:
        raise RuntimeError("invalid frame count or timeout")
    if a.artifact == "desktop" and a.verify_frames > 10000:
        raise RuntimeError("desktop verification frames cannot exceed 10000")
    out.mkdir(parents=True)
    logs = out / "logs"
    profiles = out / "profiles"
    held = out / "artifacts"
    saves = out / "saves"
    for d in (logs, profiles, held, saves):
        d.mkdir()
    held_driver = out / "build-optimized.py"
    shutil.copy2(Path(__file__).resolve(), held_driver)
    report = {
        "schema": 1,
        "status": "running",
        "phase": "initialize",
        "artifact": a.artifact,
        "driver_sha256": digest(Path(__file__).resolve()),
        "held_driver": {"file": held_driver.name, "sha256": digest(held_driver)},
        "build_features": {
            "features_raw": a.features,
            "features": a.features.split(",") if a.features else [],
            "default_features": not a.no_default_features,
            "offline": a.offline,
        },
    }
    report_path = out / "manifest.json"
    env = os.environ.copy()
    env["ZEFF_MUTE_AUDIO"] = "1"
    try:
        forbidden = [
            k
            for k, v in env.items()
            if v
            and (
                k in {"RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS"}
                or k.startswith("CARGO_PROFILE_RELEASE_")
                or (k.startswith("CARGO_TARGET_") and k.endswith("_RUSTFLAGS"))
            )
        ]
        if forbidden:
            raise RuntimeError(
                "build-affecting environment flags must be unset: "
                + ",".join(sorted(forbidden))
            )
        inherited_training = [
            key for key in ("ZEFF_PGO_TRAINING", "ZEFF_PGO_FRAMES") if env.get(key)
        ]
        if inherited_training:
            raise RuntimeError(
                "PGO training environment must be unset: "
                + ",".join(inherited_training)
            )
        for k in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "LLVM_PROFILE_FILE"):
            env.pop(k, None)
        tc = tools(root)
        host = tc["host"]
        reject_native_config_rustflags(root, host, env)
        macos_compilers = configure_macos_c_wrappers(out, host, env, root)
        target = (a.target_dir or root / "target").resolve()
        target.mkdir(parents=True, exist_ok=True)
        env["CARGO_TARGET_DIR"] = str(target)
        if a.expected_target and a.expected_target != host:
            raise RuntimeError(
                f"native host {host} does not match expected target {a.expected_target}"
            )
        primary_name, harness_name = names(a.artifact, host)
        release = target / host / "release"
        primary = release / primary_name
        harness = release / harness_name
        feature_args = (["--features", a.features] if a.features else []) + (
            ["--no-default-features"] if a.no_default_features else []
        )
        select = (
            ["-p", "zeff-libretro", "--lib"]
            if a.artifact == "libretro"
            else ["--bin", "zeff-boy"]
        ) + feature_args
        build = ["cargo", "build", "--release", "--locked", "--target", host] + (
            ["--offline"] if a.offline else []
        )
        before = fingerprint(root)
        report.update(
            toolchain=tc,
            macos_compilers=macos_compilers,
            source_before=before,
            target_dir=str(target),
        )
        if a.corpus_manifest:
            manifests = [a.corpus_manifest.resolve()]
        else:
            report["phase"] = "corpus"
            generated = out / "corpus"
            execute(
                [
                    "cargo",
                    "run",
                    "--release",
                    "--locked",
                    "--target",
                    host,
                    *((["--offline"] if a.offline else [])),
                    "-p",
                    "zeff-pgo-corpus",
                    "--",
                    "--output-dir",
                    str(generated),
                ],
                root,
                env,
                logs / "corpus.log",
            )
            manifests = [generated / "manifest.json"]
        manifests += [p.resolve() for p in a.extra_corpus_manifest]
        fixtures = [f for m in manifests for f in corpus(m)]
        if len({f["id"] for f in fixtures}) != len(fixtures):
            raise RuntimeError("fixture ids must be unique across all corpus manifests")
        if not REQUIRED_CORES <= {f["core_crate"] for f in fixtures}:
            raise RuntimeError("corpus must cover every supported core family")
        if a.artifact == "desktop" and max(f["frames"] for f in fixtures) > 10000:
            raise RuntimeError("desktop corpus frames cannot exceed 10000")
        corpus_before = {str(f["path"]): digest(f["path"]) for f in fixtures}
        report["corpus"] = [
            {k: v for k, v in f.items() if k != "path"} for f in fixtures
        ]
        report["phase"] = "control"
        control_select = select + (
            ["--bin", "libretro_harness"] if a.artifact == "libretro" else []
        )
        execute(build + control_select, root, env, logs / "control.log")
        control = held / ("control-" + primary_name)
        shutil.copy2(primary, control)
        held_harness = held / harness_name
        if a.artifact == "libretro":
            shutil.copy2(harness, held_harness)
        report["phase"] = "instrumented"
        env["CARGO_ENCODED_RUSTFLAGS"] = profile_generate_flags()
        execute(build + select, root, env, logs / "instrumented.log")
        instrumented = held / ("instrumented-" + primary_name)
        shutil.copy2(primary, instrumented)
        report["phase"] = "training"
        env["LLVM_PROFILE_FILE"] = str(profiles / "%p-%m.profraw")
        for i, f in enumerate(fixtures):
            save = saves / f"{i:02d}-{f['id']}"
            save.mkdir()
            if a.artifact == "libretro":
                cmd = [
                    str(held_harness),
                    str(instrumented),
                    str(f["path"]),
                    "--warmup",
                    str(a.warmup),
                    "--frames",
                    str(f["frames"]),
                    "--pixel-format",
                    "xrgb8888",
                    "--save-dir",
                    str(save),
                    "--blackhole-output",
                ]
            else:
                cmd = [
                    str(instrumented),
                    "--headless",
                    "--max-frames",
                    str(a.warmup + f["frames"]),
                    "--no-sram",
                    str(f["path"]),
                ]
            training_output = execute(cmd, root, env, logs / f"training-{i:02d}.log")
            if a.artifact == "libretro" and not re.search(
                r"\baudio_frames=[1-9]\d*", training_output
            ):
                raise RuntimeError(f"training produced no audio samples: {f['id']}")
        if a.artifact == "desktop":
            coleco = env.copy()
            coleco.update(
                ZEFF_PGO_TRAINING="coleco",
                ZEFF_PGO_FRAMES=str(max(f["frames"] for f in fixtures)),
            )
            execute([str(instrumented)], root, coleco, logs / "training-coleco.log")
        env.pop("LLVM_PROFILE_FILE", None)
        raw = [p for p in profiles.glob("*.profraw") if p.stat().st_size]
        if not raw:
            raise RuntimeError("training produced no profiles")
        merged = profiles / "merged.profdata"
        execute(
            [tc["profdata"], "merge", "-o", str(merged), *map(str, raw)],
            root,
            env,
            logs / "merge.log",
        )
        ph = digest(merged)
        hashed = profiles / f"merged-{ph}.profdata"
        shutil.copy2(merged, hashed)
        covered = execute(
            [
                tc["profdata"],
                "show",
                "--covered",
                "--all-functions",
                "--counts",
                str(hashed),
            ],
            root,
            env,
            logs / "coverage.log",
            quiet=True,
        )
        expected = sorted(
            {f["core_crate"] for f in fixtures}
            | ({"zeff_coleco_core"} if a.artifact == "desktop" else set())
        )
        if missing := [c for c in expected if c not in covered]:
            raise RuntimeError("missing covered crates: " + ",".join(missing))
        if (
            fingerprint(root) != before
            or tools(root) != tc
            or {str(f["path"]): digest(f["path"]) for f in fixtures} != corpus_before
        ):
            raise RuntimeError(
                "source, toolchain, or corpus changed before profile use"
            )
        report["phase"] = "optimized"
        env["CARGO_ENCODED_RUSTFLAGS"] = (
            f"-Cprofile-use={hashed}\x1f-Cllvm-args=-pgo-warn-missing-function"
        )
        report["profile_diagnostics"] = {}
        execute(build + select, root, env, logs / "optimized.log", True,
                diagnostics=report["profile_diagnostics"])
        optimized = held / ("optimized-" + primary_name)
        shutil.copy2(primary, optimized)
        if (
            fingerprint(root) != before
            or tools(root) != tc
            or {str(f["path"]): digest(f["path"]) for f in fixtures} != corpus_before
        ):
            raise RuntimeError(
                "source, toolchain, or corpus changed during profile use"
            )
        report["phase"] = "verification"
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        verified = []
        for i, f in enumerate(fixtures):
            frames = min(a.verify_frames, f["frames"])
            if a.artifact == "libretro":
                control_save = saves / f"verify-control-{i:02d}"
                optimized_save = saves / f"verify-optimized-{i:02d}"
                control_save.mkdir()
                optimized_save.mkdir()
                common = [
                    str(f["path"]),
                    "--frames",
                    str(frames),
                    "--repeat",
                    "2",
                    "--continuation-frames",
                    str(a.continuation_frames),
                    "--pixel-format",
                    "xrgb8888",
                ]
                left = execute(
                    [
                        str(held_harness),
                        str(control),
                        *common,
                        "--save-dir",
                        str(control_save),
                    ],
                    root,
                    env,
                    logs / f"verify-control-{i}.log",
                )
                right = execute(
                    [
                        str(held_harness),
                        str(optimized),
                        *common,
                        "--save-dir",
                        str(optimized_save),
                    ],
                    root,
                    env,
                    logs / f"verify-optimized-{i}.log",
                )
                left_summary = semantics(left)
                right_summary = semantics(right)
                if left_summary != right_summary:
                    raise RuntimeError(f"output mismatch: {f['id']}")
                strict = (
                    left_summary.get("continuation_match") == "true"
                    and right_summary.get("continuation_match") == "true"
                )
            else:
                control_png = out / f"verify-control-{i:02d}.png"
                optimized_png = out / f"verify-optimized-{i:02d}.png"
                common = [
                    "--headless",
                    "--max-frames",
                    str(frames),
                    "--no-sram",
                    str(f["path"]),
                ]
                control_output = execute(
                    [str(control), *common, "--screenshot", str(control_png)],
                    root,
                    env,
                    logs / f"verify-control-{i}.log",
                )
                optimized_output = execute(
                    [str(optimized), *common, "--screenshot", str(optimized_png)],
                    root,
                    env,
                    logs / f"verify-optimized-{i}.log",
                )
                if not re.search(
                    rf"\bframes={frames}\b", control_output
                ) or not re.search(rf"\bframes={frames}\b", optimized_output):
                    raise RuntimeError(f"desktop completion frame mismatch: {f['id']}")
                if not control_png.is_file() or digest(control_png) != digest(
                    optimized_png
                ):
                    raise RuntimeError(f"desktop screenshot mismatch: {f['id']}")
            verified.append(
                {
                    "id": f["id"],
                    "frames": frames,
                    "baseline_equal": True,
                    "strict_continuation": strict if a.artifact == "libretro" else None,
                    "evidence": (
                        "full harness domains"
                        if a.artifact == "libretro"
                        else "encoded PNG equality"
                    ),
                }
            )
        if a.artifact == "desktop":
            coleco_env = env.copy()
            coleco_env.update(
                ZEFF_PGO_TRAINING="coleco", ZEFF_PGO_FRAMES=str(a.verify_frames)
            )
            left = execute(
                [str(control)], root, coleco_env, logs / "verify-control-coleco.log"
            )
            right = execute(
                [str(optimized)], root, coleco_env, logs / "verify-optimized-coleco.log"
            )
            try:
                left_json = json.loads(left)
                right_json = json.loads(right)
            except json.JSONDecodeError as error:
                raise RuntimeError("Coleco verification did not emit JSON") from error
            if left_json != right_json:
                raise RuntimeError("Coleco synthetic output mismatch")
            verified.append(
                {
                    "id": "coleco-synthetic",
                    "frames": a.verify_frames,
                    "baseline_equal": True,
                    "strict_continuation": None,
                    "evidence": "training JSON",
                }
            )
        report.update(
            status="complete",
            phase="complete",
            profile={"sha256": ph, "raw_files": len(raw), "covered_crates": expected},
            artifacts={
                "control": {"file": control.name, "sha256": digest(control)},
                "optimized": {"file": optimized.name, "sha256": digest(optimized)},
            },
            verification=verified,
            source_final=fingerprint(root),
        )
    except Exception as error:
        report.update(status="failed", error=str(error))
        raise
    finally:
        report_path.write_text(
            json.dumps(report, indent=2, sort_keys=True, default=str) + "\n",
            encoding="utf-8",
        )
    print(optimized)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
