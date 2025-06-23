{
  description = "Pruebas";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
          config.allowUnfree = true;
        };
        rust-bin-custom = pkgs.rust-bin.stable.latest.default.override {
          extensions = ["rust-src"];
        };

        run-raft = pkgs.writeShellScriptBin "run-raft" ''
          set -e
            echo "Building Rust Raft implementation..."
            cargo fmt
            cargo build

              if [ $? -ne 0 ]; then
              echo "❌ Main Raft build failed!"
              exit 1
            fi

            echo "Starting visual frontend..."
            cd interactive
            cargo run
        '';
      in {
        devShells.default = with pkgs;
          mkShell {
            name = "Pruebas";
            buildInputs = [
              cargo-watch
              capnproto
              pkg-config
              rust-bin-custom
              run-raft
            ];
          };
      }
    );
}
