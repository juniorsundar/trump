{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
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

          static = if pkgs.stdenv.isLinux then
            (pkgs.pkgsMusl.callPackage ./nix/package.nix { }).overrideAttrs (old: {
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
            '';
          };
        }
      );
    };
}