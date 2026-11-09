{
  description = "gh-board - a GitHub Projects board in the terminal";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/26.05";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # cargo, rustc, clippy, rustfmt and rust-analyzer, pinned in
            # rust-toolchain.toml.
            (rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
            cargo-watch
            cargo-insta
            lldb
            gh # the app's transport
          ];
        };
      });
}
