"""Fetch pinned Mega Drive diagnostics; optionally build the exhaustive BCD ROM."""

import argparse
import hashlib
from html.parser import HTMLParser
import http.cookiejar
import io
import json
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import urllib.parse
import urllib.request
import zipfile


LIMIT = 16 * 1024 * 1024
ROOT = Path(__file__).resolve().parents[1]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_response(response):
    data = response.read(LIMIT + 1)
    if len(data) > LIMIT:
        raise ValueError("oversized diagnostic download")
    return data


def cached(path, expected, download):
    if path.exists():
        with path.open("rb") as source:
            data = read_response(source)
    else:
        data = download()
    if len(data) > LIMIT:
        raise ValueError("oversized diagnostic cache input")
    if digest(data) != expected:
        raise ValueError(f"diagnostic hash mismatch: {path.name}")
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    return data


def get(url):
    with urllib.request.urlopen(url, timeout=30) as response:
        return read_response(response)


def extract(data, destination, prefix=None):
    destination = destination.resolve()
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        if sum(entry.file_size for entry in archive.infolist()) > 4 * LIMIT:
            raise ValueError("oversized diagnostic archive")
        for entry in archive.infolist():
            name = PurePosixPath(entry.filename)
            if name.is_absolute() or ".." in name.parts or "\\" in entry.orig_filename or ":" in entry.orig_filename:
                raise ValueError("unsafe diagnostic archive path")
            if entry.is_dir():
                continue
            if entry.external_attr >> 16 & 0o170000 == 0o120000:
                raise ValueError("diagnostic archive contains symlink")
            if prefix:
                name = name.relative_to(prefix)
            target = destination.joinpath(*name.parts).resolve()
            if not target.is_relative_to(destination):
                raise ValueError("diagnostic archive escapes cache")
            payload = archive.read(entry)
            if target.exists() and target.read_bytes() != payload:
                raise ValueError(f"existing diagnostic source differs: {name}")
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(payload)


class CsrfParser(HTMLParser):
    token = None

    def handle_starttag(self, tag, attrs):
        fields = dict(attrs)
        if tag == "meta" and fields.get("name") == "csrf_token":
            self.token = fields.get("content") or fields.get("value")


def itch_file(suite):
    opener = urllib.request.build_opener(
        urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    with opener.open(suite["page"], timeout=30) as response:
        parser = CsrfParser()
        parser.feed(read_response(response).decode("utf-8"))
    if not parser.token:
        raise ValueError("official download page has no anonymous request token")
    request = urllib.request.Request(
        suite["page"] + "/file/" + str(suite["upload_id"]),
        data=urllib.parse.urlencode({"csrf_token": parser.token}).encode(),
        headers={"Referer": suite["page"], "X-Requested-With": "XMLHttpRequest"})
    with opener.open(request, timeout=30) as response:
        result = json.loads(read_response(response))
    url = result.get("url")
    parts = urllib.parse.urlsplit(url) if isinstance(url, str) else None
    if not parts or parts.scheme != "https" or not parts.hostname:
        raise ValueError("official download did not return an HTTPS file URL")
    with opener.open(url, timeout=30) as response:
        return read_response(response)


def build_bcd(bcd, source, cache):
    if not shutil.which("g++"):
        raise ValueError("BCD build requires g++ in PATH")
    assembler = bcd["assembler"]
    archive = cached(cache / "asl-windows.zip", assembler["sha256"],
                     lambda: get(assembler["url"]))
    tools = cache / "asl-windows"
    extract(archive, tools)
    commands = [
        ["g++", "-O3", "-s", "-Wall", "-Wextra", "-pedantic", "-Werror", "-std=c++11",
         "-o", "bcd-gen.exe", "bcd-gen.cc", "bcd-emul.cc"],
        [str(source / "bcd-gen.exe")],
        [str(tools / "asw.exe"), "-xx", "-c", "-L", "-r", "2", "-A", "-U", "bcd-verifier.asm"],
        [str(tools / "p2bin.exe"), "-r", "0-$", "bcd-verifier.p"],
    ]
    with (cache / "bcd-build.log").open("w", encoding="utf-8") as log:
        for command in commands:
            subprocess.run(command, cwd=source, stdout=log, stderr=subprocess.STDOUT,
                           check=True, timeout=60)
    binary = (source / "bcd-verifier.bin").read_bytes()
    if digest(binary) != bcd["rom_sha256"]:
        raise ValueError("built BCD ROM does not match pinned result")
    if digest((source / "data/bcd-table.bin").read_bytes()) != bcd["table_sha256"]:
        raise ValueError("built BCD table does not match pinned witnesses")
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-bcd", action="store_true",
                        help="Windows build using pinned AS and local g++")
    args = parser.parse_args()
    catalog = json.loads((ROOT / "rom-tests/md-diagnostics.json").read_text())
    if catalog["schema"] != 1:
        raise ValueError("unsupported diagnostic catalogue schema")
    cache = (ROOT / "rom-tests/cache/md-diagnostics").resolve()
    cache.mkdir(parents=True, exist_ok=True)
    bcd = catalog["bcd_verifier"]
    archive = cached(cache / "bcd-source.zip", bcd["archive_sha256"], lambda: get(bcd["source"]))
    source = cache / "bcd-source"
    extract(archive, source, "68k-bcd-verifier-" + bcd["commit"])
    for entry in bcd["files"]:
        if digest((source / entry["path"]).read_bytes()) != entry["sha256"]:
            raise ValueError("BCD source member hash mismatch")
    suite = catalog["suite_240p"]
    archive = cached(cache / "240p-genesis-1.32.zip", suite["archive_sha256"], lambda: itch_file(suite))
    suite_path = cache / "240p"
    extract(archive, suite_path)
    for name, expected in suite["files"].items():
        if digest((suite_path / name).read_bytes()) != expected:
            raise ValueError("240p diagnostic member hash mismatch")
    receipt = {"schema": 1, "catalog_sha256": digest((ROOT / "rom-tests/md-diagnostics.json").read_bytes()),
               "suite_240p": {"version": suite["version"], "files": suite["files"]},
               "bcd": {"source_commit": bcd["commit"], "built": args.build_bcd},
               "claim": "diagnostic intake only; ROM execution and new physical hardware validation pending"}
    if args.build_bcd:
        binary = build_bcd(bcd, source, cache)
        (cache / "bcd-verifier.bin").write_bytes(binary)
        receipt["bcd"].update({"rom_sha256": digest(binary), "bytes": len(binary),
                               "table_sha256": bcd["table_sha256"], "table_witnesses": bcd["table_witnesses"]})
    (cache / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
