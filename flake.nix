{
  description = "JSON CLI for a running QueryMT ACP WebSocket server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = inputs @ {self, ...}:
    inputs.flake-parts.lib.mkFlake {inherit inputs;} {
      systems = inputs.nixpkgs.lib.systems.flakeExposed;

      perSystem = {system, ...}: let
        overlays = [inputs.rust-overlay.overlays.default];
        pkgs = import inputs.nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

        qmtacp = pkgs.rustPlatform.buildRustPackage {
          pname = "qmtacp";
          version = cargoToml.package.version;
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          auditable = false;
          doCheck = false;
        };
      in {
        packages = {
          qmtacp = qmtacp;
          default = qmtacp;
        };

        apps = {
          qmtacp = {
            type = "app";
            program = "${self.packages.${system}.qmtacp}/bin/qmtacp";
          };
          default = {
            type = "app";
            program = "${self.packages.${system}.qmtacp}/bin/qmtacp";
          };
        };

        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            pkgs.pkg-config
            pkgs.openssl
          ];

          shellHook = ''
            export PS1="(dev:qmtacp) $PS1"
          '';
        };
      };
    };
}
