#!/usr/bin/env python3
import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import pwd
import secrets
import signal
import socket
import socketserver
import subprocess
import sys
import threading
import time
import urllib.request


def run(*args):
    return subprocess.check_output(child_command(args), text=True, timeout=10)


def child_command(args):
    return ["/usr/bin/env", "--default-signal=HUP,INT,TERM", "--", *args]


def copy_stream(source, destination):
    try:
        while data := source.recv(16384):
            destination.sendall(data)
    except OSError:
        pass
    finally:
        try:
            destination.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def bridge(source, destination):
    returning = threading.Thread(target=copy_stream, args=(destination, source), daemon=True)
    returning.start()
    copy_stream(source, destination)
    returning.join(timeout=1)


class UnixBridge(socketserver.ThreadingUnixStreamServer):
    daemon_threads = True


class LoopbackProxy(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


def proxy(unix_path, port, ready):
    class Handler(socketserver.BaseRequestHandler):
        def handle(self):
            with socket.socket(socket.AF_UNIX) as upstream:
                upstream.connect(str(unix_path))
                bridge(self.request, upstream)

    with LoopbackProxy(("127.0.0.1", port), Handler) as server:
        ready.write_text("ready\n")
        server.serve_forever(poll_interval=0.1)


def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def stop(process):
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        process.wait(timeout=5)
        return
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)


def wait_file(path, peers, deadline):
    while not path.exists():
        if any(peer.poll() is not None for peer in peers):
            raise RuntimeError(f"process exited before {path.name}")
        if time.monotonic() >= deadline:
            raise TimeoutError(f"waiting for {path.name}")
        time.sleep(0.01)


def qdisc(namespace, device):
    return json.loads(run("ip", "netns", "exec", namespace, "tc", "-s", "-j", "qdisc", "show", "dev", device))


def shaped_counts(qdiscs):
    shaped = [entry for entry in qdiscs if entry["kind"] == "netem"]
    if len(shaped) != 1:
        raise RuntimeError("netem qdisc missing")
    return {key: shaped[0].get(key, 0) for key in ("bytes", "packets", "drops")}


def check_reports(reports, args):
    for report in reports:
        for field in ("production_app", "admitted", "exact_restore", "save_protection_after_shutdown"):
            if report.get(field) is not True:
                raise RuntimeError(f"proof failed: {field}")
        for field in ("frames", "reference_checked_frames"):
            if report.get(field) != args.frames:
                raise RuntimeError(f"proof incomplete: {field}")
        if report.get("transport") != "webrtc-data-channel" or report.get("scope") != "direct-dtls-sctp":
            raise RuntimeError("proof did not use direct RTC")
        if report.get("input_delay") != args.input_delay or report.get("outcome") != "complete":
            raise RuntimeError("proof configuration or outcome differs")
        verification = report["cadence"]["verification"]
        if verification["reference_checked_frames"] != args.frames or verification["submitted_samples"] != args.frames:
            raise RuntimeError("cadence verification incomplete")
    for field in ("checkpoint", "confirmed_pcm_sha256", "final_video_sha256"):
        if reports[0][field] != reports[1][field]:
            raise RuntimeError(f"peer output differs: {field}")


def proof(args):
    if sys.platform != "linux" or os.geteuid() != 0:
        raise RuntimeError("run on Linux as root for isolated network namespaces")
    account = pwd.getpwnam(args.user)
    if account.pw_uid == 0:
        raise RuntimeError("proof processes must use an unprivileged user")
    for name in ("exe", "lobby"):
        path = getattr(args, name).resolve(strict=True)
        if not path.is_file() or not os.access(path, os.X_OK):
            raise RuntimeError(f"not executable: {name}")
        setattr(args, name, path)
    output = args.output.resolve()
    output.mkdir(mode=0o700)
    os.chown(output, account.pw_uid, account.pw_gid)
    tag = secrets.token_hex(4)
    namespaces = [f"zeff-netplay-{tag}-{role}" for role in range(2)]
    created = []
    processes = []
    logs = []
    unix_server = None
    unix_thread = None
    creating = 0
    pending_signal = None
    deadline = time.monotonic() + 120

    def interrupt(signum, _frame):
        nonlocal pending_signal
        if creating:
            pending_signal = signum
            return
        for watched in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
            signal.signal(watched, signal.SIG_IGN)
        raise KeyboardInterrupt(f"interrupted by signal {signum}")

    @contextmanager
    def track_creation():
        nonlocal creating
        creating += 1
        try:
            yield
        finally:
            creating -= 1
            if not creating and pending_signal is not None:
                interrupt(pending_signal, None)

    for watched in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(watched, interrupt)

    def launch(command, name, env=None):
        log = (output / f"{name}.log").open("wb")
        logs.append(log)
        with track_creation():
            process = subprocess.Popen(child_command(command), env=env, stdout=log,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            processes.append(process)
        return process

    def within(namespace, command):
        return ["ip", "netns", "exec", namespace, "runuser", "-u", args.user, "--", *command]

    try:
        for namespace in namespaces:
            with track_creation():
                run("ip", "netns", "add", namespace)
                created.append(namespace)
            run("ip", "-n", namespace, "link", "set", "lo", "up")
        run("ip", "-n", namespaces[0], "link", "add", "peer0", "type", "veth", "peer", "name", "peer1")
        run("ip", "-n", namespaces[0], "link", "set", "peer1", "netns", namespaces[1])
        for role, namespace in enumerate(namespaces):
            device = f"peer{role}"
            run("ip", "-n", namespace, "address", "add", f"10.237.0.{role + 1}/30", "dev", device)
            run("ip", "-n", namespace, "link", "set", device, "up")
            run("ip", "-n", namespace, "route", "add", "default", "dev", device)
            command = ["ip", "netns", "exec", namespace, "tc", "qdisc", "add", "dev", device, "root", "netem",
                       "limit", "4096", "delay", f"{args.delay_ms}ms"]
            if args.jitter_ms:
                command += [f"{args.jitter_ms}ms"]
            if args.loss_percent:
                command += ["loss", "random", f"{args.loss_percent}%"]
            run(*command)
        calibration = run("ip", "netns", "exec", namespaces[0], "ping", "-c", "3", "-i", "0.2", "-W", "2", "10.237.0.2")
        (output / "calibration.txt").write_text(calibration)
        before = [shaped_counts(qdisc(namespace, f"peer{role}")) for role, namespace in enumerate(namespaces)]

        port = free_port()
        token = secrets.token_hex(32)
        lobby_env = {key: value for key, value in os.environ.items() if not key.startswith("ZEFF_LOBBY_")}
        env = dict(lobby_env, ZEFF_LOBBY_BIND=f"127.0.0.1:{port}", ZEFF_LOBBY_PUBLIC="false",
                   ZEFF_LOBBY_ACCESS_TOKEN=token, ZEFF_LOBBY_STUN_URLS="", ZEFF_LOBBY_ALLOW_TURN="false")
        lobby = launch(["runuser", "-u", args.user, "--", str(args.lobby)], "lobby", env)
        health = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        while True:
            if lobby.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError("local lobby failed to start")
            try:
                with health.open(f"http://127.0.0.1:{port}/health", timeout=1) as response:
                    if response.status == 200:
                        break
            except OSError:
                time.sleep(0.05)

        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                with socket.create_connection(("127.0.0.1", port), timeout=10) as upstream:
                    upstream.settimeout(None)
                    bridge(self.request, upstream)

        socket_path = output / "signaling.sock"
        with track_creation():
            unix_server = UnixBridge(str(socket_path), Handler)
            os.chown(socket_path, account.pw_uid, account.pw_gid)
            socket_path.chmod(0o600)
            unix_thread = threading.Thread(target=unix_server.serve_forever, kwargs={"poll_interval": 0.1}, daemon=True)
            unix_thread.start()
        proxy_port = free_port()
        for role, namespace in enumerate(namespaces):
            ready = output / f"proxy-{role}.ready"
            process = launch(within(namespace, [sys.executable, str(Path(__file__).resolve()), "--proxy-unix",
                str(socket_path), "--proxy-port", str(proxy_port), "--proxy-ready", str(ready)]), f"proxy-{role}")
            wait_file(ready, [process], deadline)

        peers = []
        roots = [output / "host", output / "join"]
        for role, namespace in enumerate(namespaces):
            env = dict(os.environ, ZEFF_MUTE_AUDIO="1", ZEFF_CONFIG_DIR=str(roots[role] / "config"),
                ZEFF_NETPLAY_APP_ROUTE="lobby", ZEFF_NETPLAY_APP_LOBBY_URL=f"ws://127.0.0.1:{proxy_port}/v1/ws",
                ZEFF_NETPLAY_APP_LOBBY_TOKEN=token, ZEFF_NETPLAY_APP_LAN_ROLE="host" if role == 0 else "join",
                ZEFF_NETPLAY_APP_LAN_FRAMES=str(args.frames), ZEFF_NETPLAY_APP_LAN_INPUT_DELAY=str(args.input_delay),
                ZEFF_NETPLAY_APP_LAN_CADENCE="1", ZEFF_NETPLAY_APP_LAN_PACED="0", ZEFF_NETPLAY_APP_LAN_JITTER_MS="0")
            for name in ("EXPECT", "FAULT", "FAULT_ROLE", "ROM", "INVITATION"):
                env.pop(f"ZEFF_NETPLAY_APP_LAN_{name}", None)
            if role:
                wait_file(roots[0] / "invitation.txt", peers, deadline)
                env["ZEFF_NETPLAY_APP_LAN_INVITATION"] = (roots[0] / "invitation.txt").read_text().strip()
            peers.append(launch(within(namespace, [str(args.exe), "--netplay-app-proof", str(roots[role])]),
                                "host" if role == 0 else "join", env))
        for root in roots:
            wait_file(root / "ready.json", peers, deadline)
        for root in roots:
            (root / "finish").write_text("ready\n")
        for peer in peers:
            if peer.wait(timeout=max(1, deadline - time.monotonic())) != 0:
                raise RuntimeError("proof process failed")
        reports = [json.loads((root / "report.json").read_text()) for root in roots]
        check_reports(reports, args)
        after_qdiscs = [qdisc(namespace, f"peer{role}") for role, namespace in enumerate(namespaces)]
        after = [shaped_counts(qdiscs) for qdiscs in after_qdiscs]
        deltas = [{key: end[key] - start[key] for key in end} for start, end in zip(before, after)]
        if any(delta["packets"] <= 0 or delta["bytes"] <= 0 for delta in deltas):
            raise RuntimeError("RTC traffic did not traverse both shaped links")
        result = {"delay_each_way_ms": args.delay_ms, "jitter_each_way_ms": args.jitter_ms,
                  "loss_each_way_percent": args.loss_percent, "input_delay_frames": args.input_delay,
                  "frames": args.frames, "qdisc_delta": deltas, "qdiscs": after_qdiscs,
                  "peers": reports, "calibration": calibration,
                  "scope": "isolated Linux veth kernel impairment of direct RTC, headless muted NES"}
        (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    finally:
        for watched in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
            signal.signal(watched, signal.SIG_IGN)
        primary_error = sys.exc_info()[1]
        cleanup_errors = []

        def cleanup(operation):
            try:
                operation()
            except Exception as error:
                cleanup_errors.append(str(error))

        for process in reversed(processes):
            cleanup(lambda: stop(process))
        if unix_server:
            if unix_thread and unix_thread.is_alive():
                cleanup(unix_server.shutdown)
                cleanup(lambda: unix_thread.join(timeout=2))
            cleanup(unix_server.server_close)
            cleanup(lambda: Path(unix_server.server_address).unlink(missing_ok=True))
        for namespace in reversed(created):
            cleanup(lambda: run("ip", "netns", "delete", namespace))
        for log in logs:
            cleanup(log.close)
        if cleanup_errors:
            message = "cleanup failed: " + "; ".join(cleanup_errors)
            if primary_error:
                primary_error.add_note(message)
            else:
                raise RuntimeError(message)
    print(json.dumps({"result": str(output / "result.json"), "namespaces_removed": created}))


def parse_args():
    parser = argparse.ArgumentParser(description="Run the native NES cadence proof through an isolated Linux netem link.")
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--lobby", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--user")
    parser.add_argument("--frames", type=int, default=300)
    parser.add_argument("--input-delay", type=int, default=2)
    parser.add_argument("--delay-ms", type=int, default=100)
    parser.add_argument("--jitter-ms", type=int, default=0)
    parser.add_argument("--loss-percent", type=float, default=0)
    parser.add_argument("--proxy-unix", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--proxy-port", type=int, help=argparse.SUPPRESS)
    parser.add_argument("--proxy-ready", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.proxy_unix:
        if args.proxy_port is None or args.proxy_ready is None:
            parser.error("incomplete proxy options")
    else:
        if any(getattr(args, field) is None for field in ("exe", "lobby", "output", "user")):
            parser.error("--exe, --lobby, --output and --user are required")
        if not (120 <= args.frames <= 1000 and 0 <= args.input_delay <= 8 and 0 <= args.delay_ms <= 500
                and 0 <= args.jitter_ms <= args.delay_ms and 0 <= args.loss_percent <= 5):
            parser.error("proof parameters exceed supported bounds")
    return args


if __name__ == "__main__":
    args = parse_args()
    if args.proxy_unix:
        proxy(args.proxy_unix, args.proxy_port, args.proxy_ready)
    else:
        proof(args)
