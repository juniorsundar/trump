{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  nixConfig = {
    extra-substituters = [ "https://juniorsundar.cachix.org" ];
    extra-trusted-public-keys = [
      "juniorsundar.cachix.org-1:uqJiixfUhyfDgFiAscCzg26tblbQ0FmK7agtDleXM4c="
    ];
  };

  outputs =
    {
      self,
      nixpkgs,
      fenix,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.callPackage ./nix/package.nix { };

          static =
            if pkgs.stdenv.isLinux then
              (pkgs.pkgsMusl.callPackage ./nix/package.nix { }).overrideAttrs (old: {
                PKG_CONFIG_ALL_STATIC = 1;
                OPENSSL_STATIC = 1;
                RUSTFLAGS = "-C target-feature=+crt-static";
              })
            else
              null;

          static-aarch64 =
            if pkgs.stdenv.isLinux then
              (pkgs.pkgsCross.aarch64-multiplatform-musl.callPackage ./nix/package.nix { }).overrideAttrs (old: {
                PKG_CONFIG_ALL_STATIC = 1;
                OPENSSL_STATIC = 1;
                RUSTFLAGS = "-C target-feature=+crt-static";
              })
            else
              null;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          toolchain = fenix.packages.${system}.complete.toolchain;
        in
        {
          default = pkgs.mkShell {
            packages = [
              toolchain
              pkgs.uv
              pkgs.python3
              pkgs.pkg-config
              pkgs.openssl
              pkgs.libssh2
              pkgs.zlib
            ];

            shellHook = ''
              export LD_LIBRARY_PATH=${
                pkgs.lib.makeLibraryPath [
                  pkgs.openssl
                  pkgs.libssh2
                  pkgs.zlib
                ]
              }:$LD_LIBRARY_PATH


              VENV_DIR=".venv"

              if [ ! -d "$VENV_DIR" ]; then
                  echo "Creating Python virtual environment at $VENV_DIR..."
                  uv venv $VENV_DIR -p ${pkgs.python3}/bin/python
              fi

              source "$VENV_DIR/bin/activate"

              uv pip install pre-commit
              pre-commit install
            '';
          };
        }
      );
    };
}
