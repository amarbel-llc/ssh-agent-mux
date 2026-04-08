use std::{collections::BTreeSet, path::PathBuf};

use ssh_agent_lib::{
    agent::{self, Agent, ListeningSocket, Session},
    client,
    error::AgentError,
    proto::{
        Extension, Identity, Unparsed,
        extension::QueryResponse,
    },
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

/// A mock SSH agent that advertises configurable extensions via the query protocol
/// and echoes back any other extension request.
#[derive(Clone)]
struct PivyLikeMockAgent {
    advertised_extensions: Vec<String>,
}

#[ssh_agent_lib::async_trait]
impl Session for PivyLikeMockAgent {
    async fn request_identities(&mut self) -> Result<Vec<Identity>, AgentError> {
        Ok(vec![])
    }

    async fn extension(&mut self, request: Extension) -> Result<Option<Extension>, AgentError> {
        match request.name.as_str() {
            "query" => Ok(Some(Extension::new_message(QueryResponse {
                extensions: self.advertised_extensions.clone(),
            })?)),
            _ => Ok(Some(Extension {
                name: format!("{}-response", request.name),
                details: request.details,
            })),
        }
    }
}

impl Agent<MockListeningSocket> for PivyLikeMockAgent {
    fn new_session(&mut self, _socket: &tokio::net::UnixStream) -> impl Session {
        self.clone()
    }
}

async fn start_pivy_like_agent(dir: &TempDir, name: &str, extensions: Vec<String>) -> PathBuf {
    let sock_path = dir.path().join(format!("{name}.sock"));
    let listener = UnixListener::bind(&sock_path).unwrap();
    let mock_socket = MockListeningSocket { listener };
    let agent = PivyLikeMockAgent {
        advertised_extensions: extensions,
    };
    tokio::spawn(async move {
        agent::listen(mock_socket, agent).await.ok();
    });
    sock_path
}

/// Baseline: a pivy-like upstream agent correctly advertises extensions via query.
#[tokio::test]
async fn pivy_like_agent_advertises_extensions_via_query() {
    let dir = TempDir::new().unwrap();
    let pivy_sock = start_pivy_like_agent(
        &dir,
        "pivy-agent",
        vec![
            "ecdh@joyent.com".into(),
            "pivy-query@joyent.com".into(),
            "pivy-ecdh@joyent.com".into(),
            "ecdh-rebox@joyent.com".into(),
        ],
    )
    .await;

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = connect_client(&pivy_sock).await;
    let response = client
        .extension(Extension {
            name: "query".to_string(),
            details: Unparsed::from(vec![]),
        })
        .await
        .expect("query should succeed");

    let ext = response.expect("query should return an extension response");
    let query_response: QueryResponse = ext
        .parse_message()
        .expect("should parse as QueryResponse")
        .expect("extension name should match");
    assert!(query_response.extensions.contains(&"ecdh@joyent.com".to_string()));
    assert!(query_response.extensions.contains(&"pivy-query@joyent.com".to_string()));
}

/// The mux's query response must include extensions from upstream agents,
/// not just the hardcoded session-bind@openssh.com.
#[tokio::test]
async fn mux_query_response_includes_upstream_extensions() {
    let dir = TempDir::new().unwrap();
    let pivy_sock = start_pivy_like_agent(
        &dir,
        "pivy-agent",
        vec![
            "ecdh@joyent.com".into(),
            "pivy-query@joyent.com".into(),
            "pivy-ecdh@joyent.com".into(),
            "ecdh-rebox@joyent.com".into(),
        ],
    )
    .await;

    let mux_sock = dir.path().join("mux-agent.sock");
    let mux_sock_clone = mux_sock.clone();
    let pivy_sock_clone = pivy_sock.clone();
    tokio::spawn(async move {
        ssh_agent_mux::MuxAgent::run(
            &mux_sock_clone,
            [&pivy_sock_clone],
            None,
            std::time::Duration::from_secs(5),
        )
        .await
        .ok();
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut client = connect_client(&mux_sock).await;
    let response = client
        .extension(Extension {
            name: "query".to_string(),
            details: Unparsed::from(vec![]),
        })
        .await
        .expect("query should succeed");

    let ext = response.expect("query should return an extension response");
    let query_response: QueryResponse = ext
        .parse_message()
        .expect("should parse as QueryResponse")
        .expect("extension name should match");

    let extensions: BTreeSet<_> = query_response.extensions.iter().collect();
    assert!(
        extensions.contains(&"session-bind@openssh.com".to_string()),
        "mux should always include its own session-bind extension"
    );
    assert!(
        extensions.contains(&"ecdh@joyent.com".to_string()),
        "mux query should include pivy agent's ecdh extension"
    );
    assert!(
        extensions.contains(&"pivy-query@joyent.com".to_string()),
        "mux query should include pivy agent's pivy-query extension"
    );
}

/// The mux should aggregate and deduplicate extensions from multiple upstreams.
#[tokio::test]
async fn mux_query_response_aggregates_multiple_upstreams() {
    let dir = TempDir::new().unwrap();

    let agent_a = start_pivy_like_agent(
        &dir,
        "agent-a",
        vec!["ecdh@joyent.com".into(), "ext-a@example.com".into()],
    )
    .await;
    let agent_b = start_pivy_like_agent(
        &dir,
        "agent-b",
        vec!["ecdh@joyent.com".into(), "ext-b@example.com".into()],
    )
    .await;

    let mux_sock = dir.path().join("mux-agent.sock");
    let mux_sock_clone = mux_sock.clone();
    let agent_a_clone = agent_a.clone();
    let agent_b_clone = agent_b.clone();
    tokio::spawn(async move {
        ssh_agent_mux::MuxAgent::run(
            &mux_sock_clone,
            [&agent_a_clone, &agent_b_clone],
            None,
            std::time::Duration::from_secs(5),
        )
        .await
        .ok();
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut client = connect_client(&mux_sock).await;
    let response = client
        .extension(Extension {
            name: "query".to_string(),
            details: Unparsed::from(vec![]),
        })
        .await
        .expect("query should succeed");

    let ext = response.expect("query should return an extension response");
    let query_response: QueryResponse = ext
        .parse_message()
        .expect("should parse as QueryResponse")
        .expect("extension name should match");

    let extensions: BTreeSet<_> = query_response.extensions.iter().collect();
    assert!(extensions.contains(&"session-bind@openssh.com".to_string()));
    assert!(extensions.contains(&"ecdh@joyent.com".to_string()), "shared extension should appear");
    assert!(extensions.contains(&"ext-a@example.com".to_string()), "agent-a extension");
    assert!(extensions.contains(&"ext-b@example.com".to_string()), "agent-b extension");
    // Deduplicated: ecdh@joyent.com appears in both but should only be listed once
    assert_eq!(
        query_response
            .extensions
            .iter()
            .filter(|e| *e == "ecdh@joyent.com")
            .count(),
        1,
        "duplicate extensions should be deduplicated"
    );
}
