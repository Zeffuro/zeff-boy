#!/usr/bin/env python3
"""Generate PKGBUILD and .SRCINFO for the AUR zeff-boy-bin package."""

import sys
import os

def main():
    if len(sys.argv) != 4:
        print(f"Usage: {sys.argv[0]} <version> <sha256> <output-dir>")
        sys.exit(1)

    version = sys.argv[1]
    sha256 = sys.argv[2]
    outdir = sys.argv[3]

    os.makedirs(outdir, exist_ok=True)

    pkgbuild = f"""\
# Maintainer: Zeffuro <Jeffroiscool@gmail.com>
pkgname=zeff-boy-bin
pkgver={version}
pkgrel=1
pkgdesc="A Game Boy, Game Boy Color, and NES emulator written in Rust"
arch=('x86_64')
url="https://github.com/Zeffuro/zeff-boy"
license=('MIT' 'Apache-2.0')
depends=('alsa-lib' 'systemd-libs')
provides=('zeff-boy')
conflicts=('zeff-boy')
source=("zeff-boy-${{pkgver}}.tar.gz::https://github.com/Zeffuro/zeff-boy/releases/download/v${{pkgver}}/zeff-boy-v${{pkgver}}-x86_64-unknown-linux-gnu.tar.gz")
sha256sums=('{sha256}')

package() {{
    install -Dm755 "zeff-boy" "${{pkgdir}}/usr/bin/zeff-boy"
    install -Dm644 "zeff-boy.desktop" "${{pkgdir}}/usr/share/applications/zeff-boy.desktop"
    install -Dm644 "zeff-boy.png" "${{pkgdir}}/usr/share/icons/hicolor/256x256/apps/zeff-boy.png"
    install -Dm644 "LICENSE-MIT" "${{pkgdir}}/usr/share/licenses/${{pkgname}}/LICENSE-MIT"
    install -Dm644 "LICENSE-APACHE" "${{pkgdir}}/usr/share/licenses/${{pkgname}}/LICENSE-APACHE"
}}
"""

    srcinfo = f"""\
pkgbase = zeff-boy-bin
\tpkgdesc = A Game Boy, Game Boy Color, and NES emulator written in Rust
\tpkgver = {version}
\tpkgrel = 1
\turl = https://github.com/Zeffuro/zeff-boy
\tarch = x86_64
\tlicense = MIT
\tlicense = Apache-2.0
\tdepends = alsa-lib
\tdepends = systemd-libs
\tprovides = zeff-boy
\tconflicts = zeff-boy
\tsource = zeff-boy-{version}.tar.gz::https://github.com/Zeffuro/zeff-boy/releases/download/v{version}/zeff-boy-v{version}-x86_64-unknown-linux-gnu.tar.gz
\tsha256sums = {sha256}

pkgname = zeff-boy-bin
"""

    with open(os.path.join(outdir, "PKGBUILD"), "w", newline="\n") as f:
        f.write(pkgbuild)

    with open(os.path.join(outdir, ".SRCINFO"), "w", newline="\n") as f:
        f.write(srcinfo)

    print(f"Generated PKGBUILD and .SRCINFO for v{version} in {outdir}/")

if __name__ == "__main__":
    main()

