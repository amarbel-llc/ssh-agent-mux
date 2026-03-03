
default: build test

build: build-nix build-rust

build-nix:
  nix build

build-rust:
  nix develop --command cargo build

dir_build := "target"

test: test-rust test-bats

test-rust:
  TMPDIR=/tmp nix develop --command tap-dancer cargo-test -skip-empty

test-bats: build-rust
  PATH="{{justfile_directory()}}/{{dir_build}}/debug:$PATH" just zz-tests_bats/test

reinstall-local: build-nix
  ./result/bin/ssh-agent-mux --uninstall-service
  ./result/bin/ssh-agent-mux --install-service
