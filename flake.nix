{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        # Override rust with nightly build from mozilla.
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rust = pkgs.rust-bin.stable.latest.default.override { };
        rust-dev = rust.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in rec {
        # `nix develop`
        devShell = pkgs.mkShell {
          buildInputs = with pkgs; [
            age
            bashInteractive
            rust-dev
          ];
        };

        packages.default = pkgs.rustPlatform.buildRustPackage rec {
          pname = "dev";
          version = "0.2.0";

          src = ./.;
          doCheck = false;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };
        };
      });
}
