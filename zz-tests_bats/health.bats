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

function health_ndjson_emits_json_records { # @test
  # agent-timeout = 1 keeps the dead-socket probes well inside the
  # `timeout 2s` wrapper in run_ssh_agent_mux (default would be 5s).
  write_config <<-EOF
	agent-timeout = 1

	[[agents]]
	name = "fake"
	socket-path = "/tmp/does-not-exist.sock"
	EOF

  # No daemon is listening and the upstream socket does not exist, so the
  # probe checks truthfully fail and the subcommand exits 1.
  run_ssh_agent_mux health --format ndjson
  assert_failure
  assert_output --partial '{"type":"plan","count":6}'
  assert_output --partial '"description":"config valid"'
  assert_output --partial '"ok":true'
  assert_output --partial '"description":"listen socket answers"'
  assert_output --partial '"ok":false'
  assert_output --partial '"type":"summary"'
}

function health_bad_config_bails_out { # @test
  write_config <<-EOF
	not-a-real-key = true
	EOF

  run_ssh_agent_mux health --format tap
  assert_failure
  assert_output --partial "not ok 1 - config valid"
  assert_output --partial "Bail out!"
}

function health_valid_config_emits_full_plan { # @test
  # agent-timeout = 1 keeps the dead-socket probes well inside the
  # `timeout 2s` wrapper in run_ssh_agent_mux (default would be 5s).
  write_config <<-EOF
	agent-timeout = 1

	[[agents]]
	name = "fake"
	socket-path = "/tmp/does-not-exist.sock"
	EOF

  # No daemon is listening and the upstream socket does not exist, so the
  # probe checks truthfully fail and the subcommand exits 1. Task 9 adds
  # the live-daemon e2e case where these go ok.
  run_ssh_agent_mux health --format tap
  assert_failure
  assert_output --partial "TAP version 14"
  assert_output --partial "1..6"
  assert_output --partial "ok 1 - config valid"
  assert_output --partial "not ok 5 - listen socket answers"
  assert_output --partial "not ok 6 - upstream fake answers"
}
