#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <stable-release-tag>" >&2
  exit 2
fi

TAG="$1"
if [[ ! "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Snap stable releases require a vMAJOR.MINOR.PATCH tag: $TAG" >&2
  exit 2
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPOSITORY="${GITHUB_REPOSITORY:-Zeffuro/zeff-boy}"
VERSION="${TAG#v}"
ASSET="zeff-boy-${TAG}-x86_64-unknown-linux-gnu.tar.gz"
BASE_URL="https://github.com/${REPOSITORY}/releases/download/${TAG}"
LOCAL_DIR="$ROOT_DIR/snap/local"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

curl --fail --location --retry 3 --silent --show-error --proto '=https' --tlsv1.2 \
  "$BASE_URL/$ASSET" -o "$WORK_DIR/$ASSET"
curl --fail --location --retry 3 --silent --show-error --proto '=https' --tlsv1.2 \
  "$BASE_URL/SHA256SUMS.txt" -o "$WORK_DIR/SHA256SUMS.txt"

EXPECTED_SHA="$(awk -v file="$ASSET" '$2 == file { print $1 }' \
  "$WORK_DIR/SHA256SUMS.txt")"
if [[ ! "$EXPECTED_SHA" =~ ^[0-9a-fA-F]{64}$ ]]; then
  echo "SHA256SUMS.txt has no unique digest for $ASSET" >&2
  exit 1
fi
printf '%s  %s\n' "$EXPECTED_SHA" "$WORK_DIR/$ASSET" | sha256sum --check --strict

if [[ "${VERIFY_GITHUB_ATTESTATION:-0}" == "1" ]]; then
  command -v gh >/dev/null
  RELEASE_STATE="$(gh release view "$TAG" --repo "$REPOSITORY" \
    --json isDraft,isPrerelease,tagName \
    --jq 'select(.isDraft == false and .isPrerelease == false) | .tagName')"
  if [[ "$RELEASE_STATE" != "$TAG" ]]; then
    echo "$TAG is not a published stable release" >&2
    exit 1
  fi
  gh attestation verify "$WORK_DIR/$ASSET" --repo "$REPOSITORY"
fi

python3 - "$WORK_DIR/$ASSET" <<'PY'
import pathlib
import sys
import tarfile

archive = sys.argv[1]
with tarfile.open(archive, "r:gz") as members:
    for member in members.getmembers():
        path = pathlib.PurePosixPath(member.name)
        if path.is_absolute() or ".." in path.parts:
            raise SystemExit(f"unsafe archive path: {member.name}")
        if not (member.isfile() or member.isdir()):
            raise SystemExit(
                f"archive member must be a regular file or directory: {member.name}"
            )
PY

rm -rf "$LOCAL_DIR"
mkdir -p "$LOCAL_DIR/payload"
tar -xzf "$WORK_DIR/$ASSET" -C "$LOCAL_DIR/payload"
printf '%s\n' "$VERSION" > "$LOCAL_DIR/version"

for file in zeff-boy zeff-boy.desktop zeff-boy.png LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.md; do
  test -f "$LOCAL_DIR/payload/$file"
done
chmod 0755 "$LOCAL_DIR/payload/zeff-boy"
