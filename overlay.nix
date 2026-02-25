final: prev: {
  rustToolchain = final.rust-bin.stable.latest.default.override {
    extensions = [ "rust-src" "rust-analyzer" ];
    targets = [ "wasm32-unknown-unknown" ];
  };

  freenet-shared = final.callPackage ./package.nix { };
}
