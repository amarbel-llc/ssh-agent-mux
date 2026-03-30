{
  description = "Combine keys from multiple SSH agents into a single agent socket";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/4590696c8693fea477850fe379a01544293ca4e2";
    nixpkgs-master.url = "github:NixOS/nixpkgs/e2dde111aea2c0699531dc616112a96cd55ab8b5";
    utils.url = "https://flakehub.com/f/numtide/flake-utils/0.1.102";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    bob.url = "github:amarbel-llc/bob";
    bob.inputs.nixpkgs.follows = "nixpkgs";
    bob.inputs.nixpkgs-master.follows = "nixpkgs-master";
    bob.inputs.utils.follows = "utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-master,
      utils,
      rust-overlay,
      bob,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
        pkgs-master = import nixpkgs-master {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        rustToolchain = pkgs-master.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "rustfmt"
          ];
        };
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "ssh-agent-mux";
          version = "0.1.6";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
            outputHashes = {
              "ssh-agent-lib-0.4.0" = "sha256-R6GIPJkgAKuOUSRPIVCRM05oIzOMdqvq6yHdKd3Vyrs=";
            };
          };

          nativeCheckInputs = [ pkgs.openssh ];

          # Skip integration tests in sandbox due to macOS SDK restrictions.
          # Tests work fine in nix develop, but fail in the stricter nix build sandbox
          # on macOS due to environment and filesystem restrictions.
          doCheck = !pkgs.stdenv.hostPlatform.isDarwin;

          meta = with pkgs.lib; {
            description = "Combine keys from multiple SSH agents into a single agent socket";
            homepage = "https://github.com/friedenberg/ssh-agent-mux";
            license = with licenses; [
              asl20
              bsd3
            ];
            maintainers = [ ];
          };
        };

        devShells.default = pkgs-master.mkShell {
          packages = [
            rustToolchain
            pkgs-master.cargo-deny
            pkgs-master.cargo-edit
            pkgs-master.cargo-watch
            pkgs-master.rust-analyzer
            pkgs.openssl
            pkgs.pkg-config
            pkgs.just
            bob.packages.${system}.batman
            bob.packages.${system}.tap-dancer
          ];
        };
      }
    );
}
