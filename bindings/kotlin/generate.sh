#!/usr/bin/env bash
# Builds keel-ffi for this host and writes its Kotlin bindings into build/generated/uniffi, with
# UniFFI's bindgen on the workspace's version (ADR-0025). The Gradle build runs it first.
#
#   bindings/kotlin/generate.sh [--release]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
profile=debug
flags=()
if [[ "${1:-}" == "--release" ]]; then
  profile=release
  flags=(--release)
fi
cd "$root"
cargo build -q -p keel-ffi --lib "${flags[@]}"
case "$(uname -s)" in
  Darwin) library="target/$profile/libkeel_ffi.dylib" ;;
  *) library="target/$profile/libkeel_ffi.so" ;;
esac
out="$here/build/generated/uniffi"
rm -rf "$out"
cargo run -q -p keel-ffi --features bindgen --bin uniffi-bindgen "${flags[@]}" -- \
  generate --library "$library" --language kotlin --out-dir "$out" --no-format \
  --config core/crates/keel-ffi/uniffi.toml
echo "$out"
