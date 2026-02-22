{
  description = "Combine keys from multiple SSH agents into a single agent socket";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/6d41bc27aaf7b6a3ba6b169db3bd5d6159cfaa47";
    utils.url = "https://flakehub.com/f/numtide/flake-utils/0.1.102";
    devenv-rust.url = "github:amarbel-llc/eng?dir=devenvs/rust";
    devenv-rust.inputs.nixpkgs.follows = "nixpkgs";
    devenv-rust.inputs.utils.follows = "utils";
  };

  outputs =
    { self
    , nixpkgs
    , utils
    , devenv-rust
    ,
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

        devShells.default = devenv-rust.devShells.${system}.default;
      }
    );
}
