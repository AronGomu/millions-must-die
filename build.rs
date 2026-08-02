//! Propagate pinned SDL3 link/search paths for the app binary.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=MMD_NATIVE_CACHE");
    println!("cargo:rerun-if-env-changed=MMD_SDL3_PREFIX");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=PATH");

    if let Some(prefix) = resolve_sdl3_prefix() {
        configure_prefix(&prefix);
    }
}

fn configure_prefix(prefix: &Path) {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "windows" => configure_windows(prefix),
        "macos" => configure_unix_like(prefix, false),
        _ => configure_unix_like(prefix, true),
    }
}

fn configure_unix_like(prefix: &Path, linux_rpath: bool) {
    let lib = first_existing(&[prefix.join("lib64"), prefix.join("lib")]);
    if !lib.is_dir() {
        return;
    }
    println!("cargo:rustc-link-search=native={}", lib.display());
    if linux_rpath {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
        println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
    } else {
        // macOS: rpath to prefix lib for dev runs.
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    }
    emit_pkg_config_path(&lib.join("pkgconfig"));
}

fn configure_windows(prefix: &Path) {
    // CMake install layout: lib holds import lib; bin holds SDL3.dll.
    let lib = first_existing(&[prefix.join("lib"), prefix.join("lib64")]);
    let bin = prefix.join("bin");
    if lib.is_dir() {
        println!("cargo:rustc-link-search=native={}", lib.display());
    }
    if bin.is_dir() {
        println!("cargo:rustc-link-search=native={}", bin.display());
        // Dev run: put SDL3.dll on PATH for the test/run process via rustc-env is
        // insufficient for child PATH; document + emit absolute dir for wrappers.
        println!("cargo:rustc-env=MMD_SDL3_BIN={}", bin.display());
    }
    let pc = first_existing(&[lib.join("pkgconfig"), prefix.join("lib/pkgconfig")]);
    if pc.is_dir() {
        emit_pkg_config_path(&pc);
    }
}

fn emit_pkg_config_path(pc: &Path) {
    if !pc.is_dir() {
        return;
    }
    let mut path = pc.display().to_string();
    if let Ok(existing) = env::var("PKG_CONFIG_PATH")
        && !existing.is_empty()
    {
        // Windows pkg-config uses `;` occasionally; keep `:` for MSYS/pkgconf.
        path = format!("{path}:{existing}");
    }
    println!("cargo:rustc-env=PKG_CONFIG_PATH={path}");
}

fn resolve_sdl3_prefix() -> Option<PathBuf> {
    if let Ok(p) = env::var("MMD_SDL3_PREFIX") {
        let pb = PathBuf::from(p);
        if pb.is_dir() {
            return Some(pb);
        }
    }

    let cache_root = cache_root();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "linux".into());
    let suffix = match target_os.as_str() {
        "windows" => "prefix-windows",
        "macos" => "prefix-macos",
        _ => "prefix-linux",
    };
    let pinned = cache_root.join("sdl3/3.4.12").join(suffix);
    pinned.is_dir().then_some(pinned)
}

fn cache_root() -> PathBuf {
    if let Ok(v) = env::var("MMD_NATIVE_CACHE") {
        return PathBuf::from(v);
    }
    // Windows native cache default uses USERPROFILE.
    if let Ok(home) = env::var("HOME").or_else(|_| env::var("USERPROFILE")) {
        return PathBuf::from(home).join(".cache/mmd/native");
    }
    PathBuf::from(".cache/mmd/native")
}

fn first_existing(paths: &[PathBuf]) -> PathBuf {
    for p in paths {
        if p.is_dir() {
            return p.clone();
        }
    }
    paths[0].clone()
}
