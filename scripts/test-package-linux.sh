#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

for tool in gcc dpkg-deb lintian rpmbuild rpm; do
  if ! command -v "$tool" >/dev/null; then
    echo "missing packaging test dependency: $tool" >&2
    exit 2
  fi
done

printf 'int main(void) { return 0; }\n' > "$WORK_DIR/stand-in.c"
gcc -O2 -s -o "$WORK_DIR/zeff-boy" "$WORK_DIR/stand-in.c"

check_packages() {
  local version="$1"
  local deb_name="$2"
  local rpm_name="$3"
  local output_dir="$WORK_DIR/packages-${version#v}"
  local deb="$output_dir/$deb_name"
  local rpm="$output_dir/$rpm_name"

  bash "$ROOT_DIR/scripts/package-linux.sh" \
    "$version" "$WORK_DIR/zeff-boy" "$output_dir"
  test -s "$deb"
  test -s "$rpm"

  local deb_contents
  deb_contents="$(dpkg-deb --contents "$deb")"
  grep -Eq '[.]/usr/games/zeff-boy$' <<<"$deb_contents"
  grep -Eq '[.]/usr/share/man/man6/zeff-boy[.]6[.]gz$' <<<"$deb_contents"
  grep -Eq '[.]/usr/share/doc/zeff-boy/changelog[.]gz$' <<<"$deb_contents"
  if grep -Eq '[.]/usr/bin/zeff-boy$|changelog[.]Debian[.]gz$' <<<"$deb_contents"; then
    echo "Debian package contains a disallowed binary or changelog path" >&2
    exit 1
  fi

  local desktop_contents
  desktop_contents="$(
    dpkg-deb --fsys-tarfile "$deb" \
      | tar -xOf - ./usr/share/applications/zeff-boy.desktop
  )"
  grep -Fxq 'Exec=/usr/games/zeff-boy %f' <<<"$desktop_contents"

  local rpm_contents
  rpm_contents="$(rpm -qlp "$rpm")"
  grep -Fxq '/usr/bin/zeff-boy' <<<"$rpm_contents"
  grep -Eq '^/usr/share/man/man6/zeff-boy[.]6([.]gz)?$' <<<"$rpm_contents"

  lintian --pedantic --fail-on warning "$deb"
}

check_packages \
  v0.3.0 \
  zeff-boy_0.3.0_amd64.deb \
  zeff-boy-0.3.0-1.x86_64.rpm
check_packages \
  v0.0.0-test.1 \
  zeff-boy_0.0.0-test.1_amd64.deb \
  zeff-boy-0.0.0-0.1.test.1.x86_64.rpm
