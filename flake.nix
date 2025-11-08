{
  description = "Combine keys from multiple SSH agents into a single agent socket";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    devenv-rust.url = "github:friedenberg/eng?dir=pkgs/alfa/devenv-rust";
  };

  outputs =
    {
      self,
      nixpkgs,
      utils,
      rust-overlay,
      devenv-rust,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];

        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable."1.81.0".default;
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "ssh-agent-mux";
          version = "0.1.6";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            rustToolchain
          ];

          nativeCheckInputs = [ pkgs.openssh ];

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
          buildInputs = with pkgs; [
            rustToolchain
            rust-analyzer
            cargo-edit
            cargo-watch
          ];

          inputsFrom = [
            devenv-rust.devShells.${system}.default
          ];

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      }
    );
}
