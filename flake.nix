{
  description = "Millions Must Die — phase-0 toolchain (Rust 1.95.0)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rustToolchain = pkgs.rust-bin.stable."1.95.0".default.override {
          extensions = [
            "rustfmt"
            "clippy"
            "rust-src"
          ];
        };
      in
      {
        formatter = pkgs.nixfmt;

        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            pkgs.pkg-config
          ];
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };

        checks.rustc-version = pkgs.runCommand "rustc-version-1.95.0" {
          nativeBuildInputs = [ rustToolchain ];
        } ''
          ver="$(rustc --version)"
          echo "$ver"
          echo "$ver" | grep -F "1.95.0" >/dev/null
          touch "$out"
        '';

        checks.toolchain-file = pkgs.runCommand "toolchain-file-1.95.0" {
          src = self;
        } ''
          grep -F 'channel = "1.95.0"' "$src/rust-toolchain.toml"
          touch "$out"
        '';
      }
    );
}
