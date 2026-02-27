use std::{ffi::OsString, io};

use harness::SshAgentInstance;

mod harness;
mod keys;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn make_openssh_agent_with_keys() -> io::Result<SshAgentInstance> {
    let agent = SshAgentInstance::new_openssh()?;
    println!("{:#?}", agent);

    for key in keys::PRIVATE {
        agent.add(key)?;
    }

    Ok(agent)
}

fn assert_no_keys_in_agent(agent: &SshAgentInstance) -> TestResult {
    let keys_in_agent = agent.list()?;
    assert!(keys_in_agent.is_empty(), "Expected no keys, got: {:?}", keys_in_agent);
    Ok(())
}

fn assert_all_keys_in_agent(agent: &SshAgentInstance) -> TestResult {
    let keys_in_agent = agent.list()?;
    for key in keys::PUBLIC {
        assert!(keys_in_agent.iter().any(|v| v == key));
    }

    Ok(())
}

#[test]
fn add_keys_to_openssh_agent() -> TestResult {
    let agent = make_openssh_agent_with_keys()?;

    assert_all_keys_in_agent(&agent)?;

    Ok(())
}

#[test]
fn empty_mux_agent() -> TestResult {
    let agent = SshAgentInstance::new_mux("", None::<OsString>)?;

    let keys_in_agent = agent.list()?;
    assert!(keys_in_agent.is_empty());

    Ok(())
}

#[test]
fn mux_with_one_agent() -> TestResult {
    let openssh_agent = make_openssh_agent_with_keys()?;
    let mux_agent = SshAgentInstance::new_mux(
        &format!(
            r##"[[agents]]
name = "upstream"
socket-path = "{}""##,
            openssh_agent.sock_path.display()
        ),
        None::<OsString>,
    )?;

    assert_all_keys_in_agent(&mux_agent)?;

    Ok(())
}

#[test]
fn mux_with_three_agents() -> TestResult {
    let agent_rsa = SshAgentInstance::new_openssh()?;
    agent_rsa.add(keys::TEST_KEY_RSA)?;
    let agent_ecdsa = SshAgentInstance::new_openssh()?;
    agent_ecdsa.add(keys::TEST_KEY_ECDSA)?;
    let agent_ed25519 = SshAgentInstance::new_openssh()?;
    agent_ed25519.add(keys::TEST_KEY_ED25519)?;

    let mux_agent = SshAgentInstance::new_mux(
        &format!(
            r##"[[agents]]
name = "rsa"
socket-path = "{}"

[[agents]]
name = "ecdsa"
socket-path = "{}"

[[agents]]
name = "ed25519"
socket-path = "{}""##,
            dbg!(&agent_rsa).sock_path.display(),
            dbg!(&agent_ecdsa).sock_path.display(),
            dbg!(&agent_ed25519).sock_path.display()
        ),
        None::<OsString>,
    )?;

    assert_all_keys_in_agent(dbg!(&mux_agent))?;

    Ok(())
}

#[test]
fn mux_add_identity_forwarding() -> TestResult {
    // Create an openssh agent to receive forwarded add_identity requests
    let target_agent = SshAgentInstance::new_openssh()?;

    // Verify the target agent is empty
    assert!(target_agent.list()?.is_empty());

    // Create a mux agent with added_keys pointing to the target agent
    let mux_agent = SshAgentInstance::new_mux(
        &format!(
            r##"add-new-keys-to = "target"

[[agents]]
name = "target"
socket-path = "{}""##,
            target_agent.sock_path.display()
        ),
        None::<OsString>,
    )?;

    // Add a key via the mux agent
    mux_agent.add(keys::TEST_KEY_RSA)?;

    // Verify the key was forwarded to the target agent
    let keys_in_target = target_agent.list()?;
    assert_eq!(keys_in_target.len(), 1);
    assert_eq!(keys_in_target[0], keys::TEST_KEY_RSA_PUB);

    Ok(())
}

#[test]
fn mux_lock_unlock() -> TestResult {
    let openssh_agent = make_openssh_agent_with_keys()?;
    let mux_agent = SshAgentInstance::new_mux(
        &format!(
            r##"[[agents]]
name = "upstream"
socket-path = "{}""##,
            openssh_agent.sock_path.display()
        ),
        None::<OsString>,
    )?;

    assert_all_keys_in_agent(&mux_agent)?;

    mux_agent.lock("test-passphrase")?;
    assert_no_keys_in_agent(&mux_agent)?;

    mux_agent.unlock("test-passphrase")?;
    assert_all_keys_in_agent(&mux_agent)?;

    Ok(())
}

#[test]
fn mux_lock_unlock_multiple_agents() -> TestResult {
    let agent_rsa = SshAgentInstance::new_openssh()?;
    agent_rsa.add(keys::TEST_KEY_RSA)?;
    let agent_ed25519 = SshAgentInstance::new_openssh()?;
    agent_ed25519.add(keys::TEST_KEY_ED25519)?;

    let mux_agent = SshAgentInstance::new_mux(
        &format!(
            r##"[[agents]]
name = "rsa"
socket-path = "{}"

[[agents]]
name = "ed25519"
socket-path = "{}""##,
            agent_rsa.sock_path.display(),
            agent_ed25519.sock_path.display()
        ),
        None::<OsString>,
    )?;

    let keys_before = mux_agent.list()?;
    assert_eq!(keys_before.len(), 2);

    mux_agent.lock("test-passphrase")?;
    assert_no_keys_in_agent(&mux_agent)?;

    // Verify upstream agents are also locked directly
    assert_no_keys_in_agent(&agent_rsa)?;
    assert_no_keys_in_agent(&agent_ed25519)?;

    mux_agent.unlock("test-passphrase")?;

    let keys_after = mux_agent.list()?;
    assert_eq!(keys_after.len(), 2);

    Ok(())
}
