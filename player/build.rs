//! Link arguments for the player's binaries.
//!
//! - Linux: let a shipped game load FFmpeg's `.so` files from its own folder
//!   (Windows does that for DLLs by itself), so players need no LD_LIBRARY_PATH.
//! - Windows: a bigger main-thread stack (8 MB instead of the default 1 MB), a safety
//!   margin for the recursive parts of the engine on top of their own depth limits.

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    match (target_os.as_str(), target_env.as_str()) {
        ("linux", _) => println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN"),
        ("windows", "msvc") => println!("cargo:rustc-link-arg-bins=/STACK:8388608"),
        ("windows", "gnu") => println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608"),
        _ => {}
    }
}
