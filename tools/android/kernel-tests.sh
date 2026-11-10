#!/usr/bin/env bash
# Runs kernel tests on an Android device or emulator (ADR-0027): each cross-built with cargo-ndk
# for the device's ABI, pushed with adb, and run there, where the kernel's platform code meets
# Android's libc, its file system and its SQLCipher build. The golden baskets are compiled into
# their test, so the binaries carry everything they need.
#
#   ABI=x86_64 tools/android/kernel-tests.sh        # needs adb, a device, and ANDROID_NDK_HOME
set -euo pipefail
abi="${ABI:-x86_64}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
remote=/data/local/tmp/keel-tests
adb shell "rm -rf $remote && mkdir -p $remote"

# Builds the test `name` of the package cargo's arguments name, and runs it on the device with
# TEST_ARGS too. cargo-ndk keeps cargo's messages to itself, so the binary is the newest of its
# name.
run() {
  local name="$1"
  shift
  cargo ndk -t "$abi" -P 28 test --release "$@" --no-run
  local triple
  case "$abi" in
    x86_64) triple=x86_64-linux-android ;;
    arm64-v8a) triple=aarch64-linux-android ;;
    *) echo "unknown ABI $abi" >&2; exit 2 ;;
  esac
  local binary
  binary=$(ls -t "${CARGO_TARGET_DIR:-$root/target}/$triple/release/deps/$name"-* | grep -v '\.d$' | head -n 1)
  adb push "$binary" "$remote/$name" > /dev/null
  echo "== $name, on the device"
  # One thread: an emulator's few cores, and stores in one temporary directory.
  adb shell "cd $remote && TMPDIR=$remote ./$name --test-threads=1 ${TEST_ARGS:-}"
}

run golden_baskets -p keel-pricing --test golden_baskets
run keel_store -p keel-store --lib
run keel_runtime -p keel-runtime --lib
run keel_ffi -p keel-ffi --lib
# The latency harness, with 100,000 more items: what it measures goes to the log.
TEST_ARGS="--ignored --nocapture" run latency -p keel-runtime --test latency
