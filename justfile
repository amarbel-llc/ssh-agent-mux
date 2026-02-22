
build: build-nix

build-nix:
  nix build

reinstall-local: build-nix
  ./result/bin/ssh-agent-mux --uninstall-service
  ./result/bin/ssh-agent-mux --install-service
