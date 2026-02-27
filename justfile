
default: build test

build: build-nix build-rust

build-nix:
  nix build

build-rust:
  nix develop --command cargo build

test: test-rust
  nix develop --command cargo test

test-rust:
  nix develop --command cargo test

reinstall-local: build-nix
  ./result/bin/ssh-agent-mux --uninstall-service
  ./result/bin/ssh-agent-mux --install-service
