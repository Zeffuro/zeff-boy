#!/usr/bin/env python3
"""Generate a WinGet multi-file manifest for a zeff-boy release."""

import argparse
import json
from pathlib import Path
import re
from urllib.request import Request, urlopen


PACKAGE_ID = "Zeffuro.ZeffBoy"
MANIFEST_VERSION = "1.12.0"


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("version", help="Release version without the leading v")
    parser.add_argument("output_root", type=Path, help="winget-pkgs checkout or staging root")
    parser.add_argument("--sha256", help="override the published Windows ZIP digest")
    return parser.parse_args()


def validate(version, sha256):
    if not re.fullmatch(r"[0-9]+(?:\.[0-9]+){2}(?:[-+][0-9A-Za-z.-]+)?", version):
        raise SystemExit(f"invalid release version: {version}")
    if not re.fullmatch(r"[0-9a-fA-F]{64}", sha256):
        raise SystemExit("sha256 must contain exactly 64 hexadecimal characters")


def published_sha256(version):
    asset_name = f"zeff-boy-v{version}-x86_64-pc-windows-msvc.zip"
    request = Request(
        f"https://api.github.com/repos/Zeffuro/zeff-boy/releases/tags/v{version}",
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "zeff-boy-winget",
        },
    )
    try:
        with urlopen(request) as response:
            release = json.load(response)
    except Exception as err:
        raise SystemExit(f"could not read published v{version} release: {err}") from err

    asset = next(
        (asset for asset in release.get("assets", []) if asset["name"] == asset_name),
        None,
    )
    if asset is None:
        raise SystemExit(f"published v{version} release has no {asset_name}")
    digest = asset.get("digest", "")
    if not digest.startswith("sha256:"):
        raise SystemExit(f"published asset has no SHA-256 digest: {asset_name}")
    return digest.removeprefix("sha256:")


def write(path, content):
    path.write_text(content, encoding="utf-8", newline="\n")


def main():
    args = parse_args()
    sha256 = args.sha256 or published_sha256(args.version)
    validate(args.version, sha256)

    version = args.version
    url = (
        "https://github.com/Zeffuro/zeff-boy/releases/download/"
        f"v{version}/zeff-boy-v{version}-x86_64-pc-windows-msvc.zip"
    )
    release_url = f"https://github.com/Zeffuro/zeff-boy/releases/tag/v{version}"
    output = (
        args.output_root
        / "manifests"
        / "z"
        / "Zeffuro"
        / "ZeffBoy"
        / version
    )
    output.mkdir(parents=True, exist_ok=True)

    schema_root = "https://aka.ms"
    write(
        output / f"{PACKAGE_ID}.yaml",
        f"""# yaml-language-server: $schema={schema_root}/winget-manifest.version.{MANIFEST_VERSION}.schema.json

PackageIdentifier: {PACKAGE_ID}
PackageVersion: {version}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: {MANIFEST_VERSION}
""",
    )
    write(
        output / f"{PACKAGE_ID}.installer.yaml",
        f"""# yaml-language-server: $schema={schema_root}/winget-manifest.installer.{MANIFEST_VERSION}.schema.json

PackageIdentifier: {PACKAGE_ID}
PackageVersion: {version}
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
- RelativeFilePath: zeff-boy.exe
  PortableCommandAlias: zeff-boy
Commands:
- zeff-boy
UpgradeBehavior: uninstallPrevious
Installers:
- Architecture: x64
  InstallerUrl: {url}
  InstallerSha256: {sha256.upper()}
ManifestType: installer
ManifestVersion: {MANIFEST_VERSION}
""",
    )
    write(
        output / f"{PACKAGE_ID}.locale.en-US.yaml",
        f"""# yaml-language-server: $schema={schema_root}/winget-manifest.defaultLocale.{MANIFEST_VERSION}.schema.json

PackageIdentifier: {PACKAGE_ID}
PackageVersion: {version}
PackageLocale: en-US
Publisher: Zeffuro
PublisherUrl: https://github.com/Zeffuro
PublisherSupportUrl: https://github.com/Zeffuro/zeff-boy/issues
Author: Zeffuro
PackageName: Zeff Boy
PackageUrl: https://github.com/Zeffuro/zeff-boy
License: MIT OR Apache-2.0
ShortDescription: A multi-system emulator written in Rust
Description: Emulates Game Boy, Game Boy Color, Game Boy Advance, NES, WonderSwan, and Sega 8-bit systems.
Moniker: zeff-boy
Tags:
- emulator
- game-boy
- game-boy-advance
- nes
- sega-master-system
- wonderswan
ReleaseNotesUrl: {release_url}
ManifestType: defaultLocale
ManifestVersion: {MANIFEST_VERSION}
""",
    )
    print(output)


if __name__ == "__main__":
    main()
