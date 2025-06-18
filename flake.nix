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
      in {
        devShells.default = with pkgs;
          mkShell {
            name = "Pruebas";
            buildInputs = [
              cargo-watch
              capnproto
              pkg-config
              cudaPackages.cudatoolkit
              cudaPackages.cuda_nvcc
              linuxPackages.nvidia_x11
              dbus
              rust-bin-custom
            ];
            LD_LIBRARY_PATH = "${pkgs.linuxPackages.nvidia_x11}/lib:${pkgs.cudaPackages.cudatoolkit}/lib";

            CUDA_ROOT = "${pkgs.cudaPackages.cudatoolkit}";
          };
      }
    );
}
