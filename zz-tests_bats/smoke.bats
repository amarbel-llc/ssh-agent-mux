#! /usr/bin/env bats

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"
  setup_test_home
  export output
}

teardown() {
  teardown_test_home
}

function help_flag_succeeds { # @test
  run_ssh_agent_mux --help
  assert_success
  assert_output --partial "ssh-agent-mux"
}

function version_flag_succeeds { # @test
  run_ssh_agent_mux --version
  assert_success
  assert_output --partial "ssh-agent-mux"
}

function validate_config_succeeds_without_config_file { # @test
  run_ssh_agent_mux config validate
  assert_success
  assert_output --partial "agents = []"
}

function install_config_creates_default_config { # @test
  # `config install` seeds the default upstream agent from SSH_AUTH_SOCK
  # (common.bash unsets the host's; provide a deterministic fake).
  export SSH_AUTH_SOCK="$BATS_TEST_TMPDIR/upstream.sock"
  run_ssh_agent_mux config install
  assert_success
  assert [ -f "$XDG_CONFIG_HOME/ssh-agent-mux/ssh-agent-mux.toml" ]
}

function validate_config_succeeds_after_install_config { # @test
  export SSH_AUTH_SOCK="$BATS_TEST_TMPDIR/upstream.sock"
  run_ssh_agent_mux config install
  assert_success

  run_ssh_agent_mux config validate
  assert_success
}
