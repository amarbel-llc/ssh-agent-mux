{
  description = "Combine keys from multiple SSH agents into a single agent socket";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/4590696c8693fea477850fe379a01544293ca4e2";
    nixpkgs-master.url = "github:NixOS/nixpkgs/ae921939fcbd44874664477bd1d22543c10a8306";
    utils.url = "https://flakehub.com/f/numtide/flake-utils/0.1.102";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # batman test orchestrator + bats helper libs (bats-support,
    # bats-assert, bats-assert-additions, bats-island). Brings its own
    # amarbel-llc/nixpkgs fork on purpose: do NOT make it follow this
    # flake's upstream nixpkgs, or the fork's auto-applied overlay
    # (fence, buildZxScriptFromFile, …) goes missing.
    bats.url = "github:amarbel-llc/bats";
    # tap-dancer (TAP wrapper used by `just test-rust`). Same rationale —
    # leave its nixpkgs on the fork.
    tap.url = "github:amarbel-llc/tap";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-master,
      utils,
      rust-overlay,
      bats,
      tap,
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
          version = "0.2.0";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
            outputHashes = {
              "ssh-agent-lib-0.5.2" = "sha256-jzbHjsYIF138FsGZioDEnCn8PpU5H7yW5FD4cU14wXw=";
            };
          };

          nativeCheckInputs = [ pkgs.openssh ];

          # Skip integration tests in sandbox due to macOS SDK restrictions.
          # Tests work fine in nix develop, but fail in the stricter nix build sandbox
          # on macOS due to environment and filesystem restrictions.
          doCheck = !pkgs.stdenv.hostPlatform.isDarwin;

          postInstall = ''
            mkdir -p $out/share/ssh-agent-mux
            cat > $out/share/ssh-agent-mux/net.ross-williams.ssh-agent-mux.plist <<EOF
            <?xml version="1.0" encoding="UTF-8"?>
            <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
            <plist version="1.0">
            <dict>
              <key>Label</key>
              <string>net.ross-williams.ssh-agent-mux</string>
              <key>ProgramArguments</key>
              <array>
                <string>$out/bin/ssh-agent-mux</string>
              </array>
              <key>KeepAlive</key>
              <true/>
              <key>RunAtLoad</key>
              <true/>
            </dict>
            </plist>
            EOF
            cat > $out/share/ssh-agent-mux/ssh-agent-mux.service <<EOF
            [Unit]
            Description=SSH Agent Multiplexer

            [Service]
            ExecStart=$out/bin/ssh-agent-mux
            Restart=on-failure

            [Install]
            WantedBy=default.target
            EOF
          '';

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
            bats.packages.${system}.batman
            tap.packages.${system}.tap-dancer
          ];
        };
      }
    );
}
