{
  description = "Combine keys from multiple SSH agents into a single agent socket";

  inputs = {
    nixpkgs-stable.url = "github:NixOS/nixpkgs/fa83fd837f3098e3e678e6cf017b2b36102c7211";
    nixpkgs.url = "github:NixOS/nixpkgs/54b154f971b71d260378b284789df6b272b49634";
    utils.url = "https://flakehub.com/f/numtide/flake-utils/0.1.102";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    devenv-rust.url = "github:friedenberg/eng?dir=pkgs/alfa/devenv-rust";
  };

  outputs =
    { self
    , nixpkgs
    , utils
    , rust-overlay
    , devenv-rust
    , nixpkgs-stable
    ,
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

          nativeBuildInputs = [
            rustToolchain
          ];

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
