#!/usr/bin/env bash
# Builds keel-ffi for this host and writes its Kotlin bindings into build/generated/uniffi, with
# UniFFI's bindgen on the workspace's version (ADR-0025), or into <dir> with --out. The Gradle
# builds, this one's and the Android app's, run it first.
#
#   bindings/kotlin/generate.sh [--release] [--out <dir>]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
profile=debug
release=""
out="$here/build/generated/uniffi"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release) profile=release; release=--release; shift ;;
    # Never empty: it is removed before it is written.
    --out) [[ -n "${2:-}" ]] || { echo "--out needs a directory" >&2; exit 2; }; out="$2"; shift 2 ;;
    *) echo "usage: $0 [--release] [--out <dir>]" >&2; exit 2 ;;
  esac
done
# Cargo's target directory, wherever CARGO_TARGET_DIR puts it.
target="${CARGO_TARGET_DIR:-$root/target}"
# A relative --out is the caller's.
[[ "$out" == /* ]] || out="$PWD/$out"
cd "$root"
# `$release` unquoted: macOS's bash 3.2 can't expand an empty array under `set -u`.
cargo build -q --locked -p keel-ffi --lib $release
case "$(uname -s)" in
  Darwin) library="$target/$profile/libkeel_ffi.dylib" ;;
  *) library="$target/$profile/libkeel_ffi.so" ;;
esac
rm -rf "$out"
cargo run -q --locked -p keel-ffi --features bindgen --bin uniffi-bindgen $release -- \
  generate --library "$library" --language kotlin --out-dir "$out" --no-format \
  --config core/crates/keel-ffi/uniffi.toml
echo "$out"
