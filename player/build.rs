//! On Linux, let a shipped game load FFmpeg's `.so` files from its own folder
//! (Windows does that for DLLs by itself), so players need no LD_LIBRARY_PATH.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
}
