{
  lib,
  makeRustPlatform,
  rustToolchain,
}:

let
  rustPlatform = makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "freenet-shared";
  version = "0.1.0";

  src = lib.cleanSource ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  # Only build the native binary tools, not the WASM contract
  cargoBuildFlags = [ "-p" "deploy-tool" "-p" "web-container-tool" ];

  meta = with lib; {
    description = "Freenet shared tools";
    license = licenses.mit;
    maintainers = [ ];
  };
}
