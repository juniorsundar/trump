{
  pkgs,
  lib,
  toolchain,
  manifest,
}:
let
  rustPlatform = pkgs.makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = manifest.name;
  version = manifest.version;

  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ pkgs.pkg-config ];

  buildInputs = [
    pkgs.openssl
    pkgs.libssh2
    pkgs.zlib
  ];

  dontStrip = false;
}
