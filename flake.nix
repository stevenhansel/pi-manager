{
  description = "Profile manager for Pi";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        pimPackage = pkgs.rustPlatform.buildRustPackage {
          pname = "pi-manager";
          version = "0.0.1";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
        };
      in
      {
        packages.default = pimPackage;
        apps.default = flake-utils.lib.mkApp { drv = pimPackage; };
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [ cargo rustc clippy rustfmt rust-analyzer cargo-audit ];
        };
      });
}
