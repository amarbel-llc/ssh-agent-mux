use std::path::PathBuf;

use ssh_agent_lib::{
    agent::{self, Agent, ListeningSocket, Session},
    client,
    error::AgentError,
    proto::{Extension, Identity, Unparsed},
};
use tempfile::TempDir;
use tokio::net::UnixListener;

/// A mock SSH agent that responds successfully to any extension request,
/// echoing back the extension name with "-response" appended.
#[derive(Clone)]
struct ExtensionEchoAgent;

#[ssh_agent_lib::async_trait]
impl Session for ExtensionEchoAgent {
    async fn request_identities(&mut self) -> Result<Vec<Identity>, AgentError> {
        Ok(vec![])
    }

    async fn extension(&mut self, request: Extension) -> Result<Option<Extension>, AgentError> {
        // Echo back the extension with a modified name to prove we received it
        Ok(Some(Extension {
            name: format!("{}-response", request.name),
            details: request.details,
        }))
    }
}

#[derive(Debug)]
struct MockListeningSocket {
    listener: UnixListener,
}

#[ssh_agent_lib::async_trait]
impl ListeningSocket for MockListeningSocket {
    type Stream = tokio::net::UnixStream;

    async fn accept(&mut self) -> std::io::Result<Self::Stream> {
        self.listener.accept().await.map(|(s, _)| s)
    }
}

impl Agent<MockListeningSocket> for ExtensionEchoAgent {
    fn new_session(&mut self, _socket: &tokio::net::UnixStream) -> impl Session {
        self.clone()
    }
}

/// Start the echo agent on a Unix socket, returning the socket path.
/// The agent runs in a background tokio task.
async fn start_echo_agent(dir: &TempDir) -> PathBuf {
    let sock_path = dir.path().join("echo-agent.sock");
    let listener = UnixListener::bind(&sock_path).unwrap();
    let mock_socket = MockListeningSocket { listener };
    tokio::spawn(async move {
        agent::listen(mock_socket, ExtensionEchoAgent).await.ok();
    });
    sock_path
}

/// Connect to an agent socket using the ssh-agent-lib client
async fn connect_client(sock_path: &PathBuf) -> Box<dyn Session> {
    let stream = tokio::net::UnixStream::connect(sock_path).await.unwrap();
    client::connect(stream.into_std().unwrap().into()).unwrap()
}

/// Verify that the echo agent itself handles custom extensions correctly
/// (baseline: if this fails, the test infrastructure is broken, not the mux)
#[tokio::test]
async fn echo_agent_handles_custom_extension() {
    let dir = TempDir::new().unwrap();
    let echo_sock = start_echo_agent(&dir).await;

    // Give the agent a moment to start listening
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = connect_client(&echo_sock).await;
    let response = client
        .extension(Extension {
            name: "pivy-query@joyent.com".to_string(),
            details: Unparsed::from(vec![]),
        })
        .await;

    let response = response.expect("echo agent should accept extension");
    let ext = response.expect("echo agent should return an extension response");
    assert_eq!(ext.name, "pivy-query@joyent.com-response");
}

/// The mux should forward unknown extensions to upstream agents.
/// Currently, the mux returns AgentError::Failure for any extension
/// that isn't "query" or "session-bind@openssh.com", which breaks
/// tools like pivy-tool that use custom SSH agent extensions.
#[tokio::test]
async fn mux_forwards_unknown_extensions_to_upstream() {
    let dir = TempDir::new().unwrap();
    let echo_sock = start_echo_agent(&dir).await;

    // Start the mux agent pointing at the echo agent
    let mux_sock = dir.path().join("mux-agent.sock");
    let mux_sock_clone = mux_sock.clone();
    let echo_sock_clone = echo_sock.clone();
    tokio::spawn(async move {
        ssh_agent_mux::MuxAgent::run(
            &mux_sock_clone,
            [&echo_sock_clone],
            None,
            std::time::Duration::from_secs(5),
        )
        .await
        .ok();
    });

    // Give both agents a moment to start listening
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut mux_client = connect_client(&mux_sock).await;
    let response = mux_client
        .extension(Extension {
            name: "pivy-query@joyent.com".to_string(),
            details: Unparsed::from(vec![]),
        })
        .await;

    // BUG: The mux currently returns Failure here instead of forwarding
    // to the upstream echo agent. Once fixed, this should succeed and
    // return the echo agent's response.
    let response = response.expect("mux should forward unknown extension to upstream, not fail");
    let ext = response.expect("upstream echo agent should return an extension response");
    assert_eq!(
        ext.name, "pivy-query@joyent.com-response",
        "extension response should come from the upstream agent"
    );
}
