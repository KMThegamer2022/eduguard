#!/usr/bin/env bash
# Build every EduGuard release binary into dist/bin/<triple>/.
#
# Usage: scripts/build-all.sh [host|macos|linux|windows|all]...
#   (no args = all)
#
# Toolchains:
#   host     — cargo only (builds for this machine)
#   macos    — cargo + lipo (universal arm64 + x86_64), macOS only
#   linux    — zig + cargo-zigbuild (glibc 2.17 baseline)
#   windows  — x86_64-w64-mingw32-gcc
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)
OUT=dist/bin
HOST=$(rustc -vV | awk '/^host:/{print $2}')
TARGETS=("$@")
[ ${#TARGETS[@]} -eq 0 ] && TARGETS=(all)

want() { [[ " ${TARGETS[*]} " == *" all "* || " ${TARGETS[*]} " == *" $1 "* ]]; }
have() { command -v "$1" >/dev/null 2>&1; }
stage() { mkdir -p "$OUT/$2"; cp "$1" "$OUT/$2/$3"; echo "  -> $OUT/$2/$3"; }

echo "EduGuard $VERSION — targets: ${TARGETS[*]}"

if want host; then
  echo "== host ($HOST)"
  cargo build --release
  stage "target/release/eduguard-server" "$HOST" eduguard-server
  stage "target/release/eduguard-agent"  "$HOST" eduguard-agent
fi

if want macos; then
  if [ "$(uname)" != "Darwin" ]; then
    echo "!! skip macos: requires a macOS host"
  else
    echo "== macOS universal (aarch64 + x86_64)"
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    mkdir -p "$OUT/universal-apple-darwin"
    for b in eduguard-server eduguard-agent; do
      lipo -create -output "$OUT/universal-apple-darwin/$b" \
        "target/aarch64-apple-darwin/release/$b" \
        "target/x86_64-apple-darwin/release/$b"
      lipo -info "$OUT/universal-apple-darwin/$b"
    done
    echo "  -> $OUT/universal-apple-darwin/{eduguard-server,eduguard-agent}"
  fi
fi

if want linux; then
  if ! have zig || ! have cargo-zigbuild; then
    echo "!! skip linux: needs zig + cargo-zigbuild (brew install zig cargo-zigbuild)"
  else
    for triple in x86_64 aarch64; do
      echo "== linux $triple (glibc 2.17)"
      # cc-rs cross C compiles (bundled SQLite, aws-lc) go through our zig
      # wrappers: zig-cc translates clang-style triples, zig-ar is LLVM ar.
      env "CC_${triple}_unknown_linux_gnu=$PWD/scripts/zig-cc" \
          "AR_${triple}_unknown_linux_gnu=$PWD/scripts/zig-ar" \
        cargo zigbuild --release --target "${triple}-unknown-linux-gnu.2.17"
      stage "target/${triple}-unknown-linux-gnu/release/eduguard-server" \
        "${triple}-unknown-linux-gnu" eduguard-server
      stage "target/${triple}-unknown-linux-gnu/release/eduguard-agent" \
        "${triple}-unknown-linux-gnu" eduguard-agent
    done
  fi
fi

if want windows; then
  if ! have x86_64-w64-mingw32-gcc; then
    echo "!! skip windows: needs mingw-w64 (brew install mingw-w64)"
  else
    echo "== windows x86_64 (mingw)"
    cargo build --release --target x86_64-pc-windows-gnu
    stage "target/x86_64-pc-windows-gnu/release/eduguard-server.exe" \
      x86_64-pc-windows-gnu eduguard-server.exe
    stage "target/x86_64-pc-windows-gnu/release/eduguard-agent.exe" \
      x86_64-pc-windows-gnu eduguard-agent.exe
  fi
fi

echo "done. binaries in $OUT/"
