#! /usr/bin/env bats

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"
  setup_test_home
  export output
}

teardown() {
  teardown_test_home
}

function health_help_succeeds { # @test
  run_ssh_agent_mux health --help
  assert_success
  assert_output --partial "--format"
}

function health_ndjson_format_not_yet_supported { # @test
  run_ssh_agent_mux health --format ndjson
  assert_failure
  assert_output --partial "not yet supported"
}
