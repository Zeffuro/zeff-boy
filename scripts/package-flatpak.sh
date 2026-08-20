#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "usage: $0 <version> <output-dir> [previous-repository-archive]" >&2
  exit 2
fi

VERSION="${1#v}"
OUTPUT_DIR="$(mkdir -p "$2" && cd "$2" && pwd)"
PREVIOUS_REPO="${3:-}"
if [[ -n "$PREVIOUS_REPO" && -f "$PREVIOUS_REPO" ]]; then
  PREVIOUS_REPO="$(realpath "$PREVIOUS_REPO")"
fi
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID="com.github.zeffuro.zeff-boy"
RUNTIME_REPO="https://dl.flathub.org/repo/flathub.flatpakrepo"
HOSTED_REPO="https://zeffuro.github.io/zeff-boy/flatpak/repo/"

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z]+)*$ ]]; then
  echo "invalid version: $VERSION" >&2
  exit 2
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"; rm -rf "$ROOT_DIR/packaging/flatpak/vendor" "$ROOT_DIR/packaging/flatpak/cargo-config.toml"' EXIT
REPO_DIR="$WORK_DIR/repo"

if [[ -z "${FLATPAK_GPG_PRIVATE_KEY:-}" && -z "${FLATPAK_GPG_KEY_ID:-}" ]]; then
  if [[ "${ALLOW_UNSIGNED_FLATPAK:-}" != "1" ]]; then
    echo "Flatpak release signing requires FLATPAK_GPG_PRIVATE_KEY and FLATPAK_GPG_KEY_ID" >&2
    exit 2
  fi
elif [[ -z "${FLATPAK_GPG_PRIVATE_KEY:-}" || -z "${FLATPAK_GPG_KEY_ID:-}" ]]; then
  echo "both FLATPAK_GPG_PRIVATE_KEY and FLATPAK_GPG_KEY_ID are required" >&2
  exit 2
fi

if [[ -n "$PREVIOUS_REPO" && -f "$PREVIOUS_REPO" ]]; then
  mkdir -p "$WORK_DIR/previous"
  tar --zstd -xf "$PREVIOUS_REPO" -C "$WORK_DIR/previous"
  if [[ -d "$WORK_DIR/previous/repo" ]]; then
    cp -a "$WORK_DIR/previous/repo" "$REPO_DIR"
  fi
fi

cd "$ROOT_DIR"
cargo vendor --locked packaging/flatpak/vendor > packaging/flatpak/cargo-config.toml

GPG_ARGS=()
GNUPG_HOME=""
if [[ -n "${FLATPAK_GPG_PRIVATE_KEY:-}" ]]; then
  GNUPG_HOME="$WORK_DIR/gnupg"
  mkdir -m700 "$GNUPG_HOME"
  printf '%s' "$FLATPAK_GPG_PRIVATE_KEY" | gpg --homedir "$GNUPG_HOME" --batch --import
  GPG_ARGS=(--gpg-sign="$FLATPAK_GPG_KEY_ID" --gpg-homedir="$GNUPG_HOME")
fi

flatpak-builder --force-clean --disable-rofiles-fuse --default-branch=stable \
  --repo="$REPO_DIR" "${GPG_ARGS[@]}" \
  "$WORK_DIR/build" packaging/flatpak/com.github.zeffuro.zeff-boy.yml

BUNDLE="$OUTPUT_DIR/zeff-boy-v${VERSION}-x86_64.flatpak"
if [[ -z "$GNUPG_HOME" ]]; then
  flatpak build-bundle --runtime-repo="$RUNTIME_REPO" \
    "$REPO_DIR" "$BUNDLE" "$APP_ID" stable
  if [[ -n "$PREVIOUS_REPO" && -f "$PREVIOUS_REPO" ]]; then
    cp "$PREVIOUS_REPO" "$OUTPUT_DIR/zeff-boy-flatpak-repo.tar.zst"
  fi
  echo "Flatpak bundle created; repository publishing skipped because no signing key was supplied"
  exit 0
fi

flatpak build-update-repo --generate-static-deltas --prune --prune-depth=2 \
  --gpg-sign="$FLATPAK_GPG_KEY_ID" --gpg-homedir="$GNUPG_HOME" "$REPO_DIR"
gpg --homedir "$GNUPG_HOME" --batch --export "$FLATPAK_GPG_KEY_ID" \
  > "$WORK_DIR/flatpak-public-key.gpg"
GPG_KEY="$(base64 --wrap=0 "$WORK_DIR/flatpak-public-key.gpg")"
flatpak build-bundle --repo-url="$HOSTED_REPO" \
  --runtime-repo="$RUNTIME_REPO" \
  --gpg-keys="$WORK_DIR/flatpak-public-key.gpg" \
  "$REPO_DIR" "$BUNDLE" "$APP_ID" stable

REF_FILE="$OUTPUT_DIR/${APP_ID}.flatpakref"
cat > "$REF_FILE" <<EOF
[Flatpak Ref]
Version=1
Name=$APP_ID
Branch=stable
Title=zeff-boy
Url=$HOSTED_REPO
RuntimeRepo=$RUNTIME_REPO
IsRuntime=false
SuggestRemoteName=zeff-boy
Homepage=https://github.com/Zeffuro/zeff-boy
GPGKey=$GPG_KEY
EOF

REPO_FILE="$OUTPUT_DIR/zeff-boy.flatpakrepo"
cat > "$REPO_FILE" <<EOF
[Flatpak Repo]
Title=zeff-boy
Url=$HOSTED_REPO
Homepage=https://github.com/Zeffuro/zeff-boy
Comment=Official zeff-boy Flatpak repository
Description=Official update repository for zeff-boy
GPGKey=$GPG_KEY
EOF

PUBLISH_DIR="$WORK_DIR/publish"
mkdir -p "$PUBLISH_DIR"
cp -a "$REPO_DIR" "$PUBLISH_DIR/repo"
cp "$REF_FILE" "$REPO_FILE" "$PUBLISH_DIR/"
tar --zstd -cf "$OUTPUT_DIR/zeff-boy-flatpak-repo.tar.zst" -C "$PUBLISH_DIR" .
