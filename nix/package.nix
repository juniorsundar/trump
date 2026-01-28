{
  lib,
  rustPlatform,
  pkg-config,
  autoPatchelfHook,
  openssl,
  libssh2,
  zlib,
  stdenv,
}:
let
  manifest = (lib.importTOML ../Cargo.toml).package;
in
rustPlatform.buildRustPackage {
  pname = manifest.name;
  version = manifest.version;

  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    autoPatchelfHook
    stdenv.cc
  ];

  buildInputs = [
    openssl
    libssh2
    zlib
    stdenv.cc.cc
  ];

  dontStrip = false;
}