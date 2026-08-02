//! Propagate pinned SDL3 rpath to the app binary.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=MMD_NATIVE_CACHE");
    println!("cargo:rerun-if-env-changed=MMD_SDL3_PREFIX");

    let prefix = env::var("MMD_SDL3_PREFIX")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            let cache = env::var("MMD_NATIVE_CACHE")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    env::var("HOME")
                        .map(|h| PathBuf::from(h).join(".cache/mmd/native"))
                        .unwrap_or_default()
                });
            let p = cache.join("sdl3/3.4.12/prefix-linux");
            p.is_dir().then_some(p)
        });

    if let Some(prefix) = prefix {
        for lib in [prefix.join("lib64"), prefix.join("lib")] {
            if lib.is_dir() {
                println!("cargo:rustc-link-search=native={}", lib.display());
                println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
                println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
                let pc = lib.join("pkgconfig");
                if pc.is_dir() {
                    let mut path = pc.display().to_string();
                    if let Ok(existing) = env::var("PKG_CONFIG_PATH") {
                        if !existing.is_empty() {
                            path = format!("{path}:{existing}");
                        }
                    }
                    println!("cargo:rustc-env=PKG_CONFIG_PATH={path}");
                }
                break;
            }
        }
    }
}
