{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
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
        packages.dev = {
          start = pkgs.writeShellScriptBin "start" ''
            echo "Starting fake webserver... Press CTRL+C to stop"
            while :; do
              sleep 3600
            done
          '';
        };

        # `nix develop`
        devShell = pkgs.mkShell {
          buildInputs = with pkgs; [
            age
            bashInteractive
            rust-dev
          ];
        };
      });
}
