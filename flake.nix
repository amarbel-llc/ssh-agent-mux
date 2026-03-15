{
  description = "Combine keys from multiple SSH agents into a single agent socket";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/3e20095fe3c6cbb1ddcef89b26969a69a1570776";
    nixpkgs-master.url = "github:NixOS/nixpkgs/e034e386767a6d00b65ac951821835bd977a08f7";
    utils.url = "https://flakehub.com/f/numtide/flake-utils/0.1.102";
    devenv-rust.url = "github:amarbel-llc/purse-first?dir=devenvs/rust";
    devenv-rust.inputs.nixpkgs.follows = "nixpkgs";
    devenv-rust.inputs.nixpkgs-master.follows = "nixpkgs-master";
    devenv-rust.inputs.utils.follows = "utils";
    purse-first.url = "github:amarbel-llc/purse-first";
    purse-first.inputs.nixpkgs.follows = "nixpkgs";
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
      devenv-rust,
      purse-first,
      bob,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
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

        devShells.default = pkgs.mkShell {
          inputsFrom = [ devenv-rust.devShells.${system}.default ];
          packages = [
            pkgs.just
            purse-first.packages.${system}.batman
            bob.packages.${system}.tap-dancer
          ];
        };
      }
    );
}
