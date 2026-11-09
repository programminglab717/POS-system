//! `UniFFI`'s bindgen, on the workspace's version: `uniffi-bindgen generate --library
//! <libkeel_ffi> --language kotlin --out-dir <dir>`.

fn main() {
    uniffi::uniffi_bindgen_main();
}
