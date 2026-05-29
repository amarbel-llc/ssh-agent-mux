//! Debug diagnostic: connect to an SSH agent socket, send the `query`
//! extension, and print the advertised extension list (one per line).
//!
//! Speaks the exact same ssh-agent-lib client path the mux uses, so it
//! doubles as a live check that the agent's real on-wire query response
//! parses (ssh-agent-mux#10).
//!
//! Usage: query-extensions <socket-path>

use ssh_agent_lib::{
    client,
    proto::{Extension, Unparsed, extension::QueryResponse},
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let sock = std::env::args()
        .nth(1)
        .expect("usage: query-extensions <socket-path>");

    let stream = tokio::net::UnixStream::connect(&sock)
        .await
        .unwrap_or_else(|e| panic!("connect {sock}: {e}"));
    let mut agent =
        client::connect(stream.into_std().unwrap().into()).expect("construct agent client");

    let response = agent
        .extension(Extension {
            name: "query".to_string(),
            details: Unparsed::from(vec![]),
        })
        .await
        .expect("query extension request");

    match response {
        Some(ext) => {
            let query_response: QueryResponse = ext
                .parse_message()
                .expect("parse QueryResponse")
                .expect("response extension name matches query");
            for extension in query_response.extensions {
                println!("{extension}");
            }
        }
        None => println!("(agent returned SSH_AGENT_SUCCESS with no extension list)"),
    }
}
