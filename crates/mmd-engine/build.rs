//! Locate pinned/system SDL3 for link + runtime rpath.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=MMD_NATIVE_CACHE");
    println!("cargo:rerun-if-env-changed=MMD_SDL3_PREFIX");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

    if let Some(prefix) = resolve_sdl3_prefix() {
        let lib = first_existing(&[prefix.join("lib64"), prefix.join("lib")]);
        let pc = lib.join("pkgconfig");
        if pc.is_dir() {
            let mut path = pc.display().to_string();
            if let Ok(existing) = env::var("PKG_CONFIG_PATH") {
                if !existing.is_empty() {
                    path = format!("{path}:{existing}");
                }
            }
            // sdl3-sys build.rs reads env at its start; also emit link search as backup.
            println!("cargo:rustc-env=PKG_CONFIG_PATH={path}");
            println!("cargo:rustc-link-search=native={}", lib.display());
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
            println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
        }
    }
}

fn resolve_sdl3_prefix() -> Option<PathBuf> {
    if let Ok(p) = env::var("MMD_SDL3_PREFIX") {
        let pb = PathBuf::from(p);
        if pb.is_dir() {
            return Some(pb);
        }
    }

    let cache_root = env::var("MMD_NATIVE_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            env::var("HOME")
                .map(|h| PathBuf::from(h).join(".cache/mmd/native"))
                .unwrap_or_else(|_| PathBuf::from(".cache/mmd/native"))
        });
    let pinned = cache_root.join("sdl3/3.4.12/prefix-linux");
    if pinned.is_dir() {
        return Some(pinned);
    }

    None
}

fn first_existing(paths: &[PathBuf]) -> PathBuf {
    for p in paths {
        if p.is_dir() {
            return p.clone();
        }
    }
    paths[0].clone()
}

#[allow(dead_code)]
fn path_exists(p: &Path) -> bool {
    p.exists()
}
