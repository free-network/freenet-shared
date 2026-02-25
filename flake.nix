{
  description = "Freenet shared tools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
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
          overlays = [
            rust-overlay.overlays.default
            self.overlays.default
          ];
        };
      in
      {
        packages = {
          freenet-shared = pkgs.freenet-shared;
          default = pkgs.freenet-shared;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ pkgs.freenet-shared ];
          packages = [
            pkgs.rustToolchain
          ];
        };
      }
    ) // {
      overlays.default = import ./overlay.nix;
    };
}
