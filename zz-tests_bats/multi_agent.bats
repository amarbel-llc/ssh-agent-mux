#! /usr/bin/env bats

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"
  setup_test_home
  export output
}

teardown() {
  teardown_test_home
}

function validate_multiple_agents { # @test
  write_config <<-EOF
	[[agents]]
	name = "default"
	socket-path = "/tmp/default.sock"

	[[agents]]
	name = "secondary"
	socket-path = "/tmp/secondary.sock"
	EOF

  run_ssh_agent_mux --validate-config
  assert_success
  assert_output --partial 'name = "default"'
  assert_output --partial 'name = "secondary"'
}

function validate_add_new_keys_to_valid_agent { # @test
  write_config <<-EOF
	add-new-keys-to = "default"

	[[agents]]
	name = "default"
	socket-path = "/tmp/default.sock"

	[[agents]]
	name = "secondary"
	socket-path = "/tmp/secondary.sock"
	EOF

  run_ssh_agent_mux --validate-config
  assert_success
  assert_output --partial 'add-new-keys-to = "default"'
}

function validate_add_new_keys_to_nonexistent_agent_fails { # @test
  write_config <<-EOF
	add-new-keys-to = "nonexistent"

	[[agents]]
	name = "default"
	socket-path = "/tmp/default.sock"
	EOF

  run_ssh_agent_mux --validate-config
  assert_failure
  assert_output --partial "add-new-keys-to references unknown agent"
}

function validate_add_new_keys_to_disabled_agent_fails { # @test
  write_config <<-EOF
	add-new-keys-to = "disabled-agent"

	[[agents]]
	name = "default"
	socket-path = "/tmp/default.sock"

	[[agents]]
	name = "disabled-agent"
	socket-path = "/tmp/disabled.sock"
	enabled = false
	EOF

  run_ssh_agent_mux --validate-config
  assert_failure
  assert_output --partial "add-new-keys-to references disabled agent"
}

function validate_add_new_keys_to_second_agent { # @test
  write_config <<-EOF
	add-new-keys-to = "secondary"

	[[agents]]
	name = "default"
	socket-path = "/tmp/default.sock"

	[[agents]]
	name = "secondary"
	socket-path = "/tmp/secondary.sock"
	EOF

  run_ssh_agent_mux --validate-config
  assert_success
  assert_output --partial 'add-new-keys-to = "secondary"'
}

function validate_disabled_agent_shown_in_output { # @test
  write_config <<-EOF
	[[agents]]
	name = "enabled-agent"
	socket-path = "/tmp/enabled.sock"

	[[agents]]
	name = "disabled-agent"
	socket-path = "/tmp/disabled.sock"
	enabled = false
	EOF

  run_ssh_agent_mux --validate-config
  assert_success
  assert_output --partial 'name = "enabled-agent"'
  assert_output --partial 'name = "disabled-agent"'
  assert_output --partial 'enabled = false'
}
