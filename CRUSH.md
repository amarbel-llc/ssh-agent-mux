# ssh-agent-mux Development Guide

## Build & Test Commands

- Build: `cargo build --release`
- Test all: `cargo test`
- Test single: `cargo test <test_name>` (e.g., `cargo test mux_with_one_agent`)
- Format: `cargo fmt`
- Lint: `cargo clippy`

## Code Style

### Imports

- Group imports: `std` first, then external crates, then internal modules
- Use multi-line format with braces for multiple items from same module
- Example: `use std::{collections::HashMap, path::PathBuf};`

### Formatting

- Indent: 4 spaces (enforced by .editorconfig)
- Use `rustfmt` defaults

### Error Handling

- Use `Result<T, AgentError>` for library functions
- Log errors before continuing when iterating over multiple sockets
- Use `log::warn!()` for recoverable errors, `log::error!()` for critical failures
- Continue processing other sockets when one fails (don't propagate with `?`)

### Async/Concurrency

- Use `tokio` runtime with `async/await`
- Use `Arc<Mutex<T>>` for shared state across async tasks

### Naming

- Use snake_case for functions and variables
- Use descriptive names: `agent_sock_path`, `known_keys`, `upstream_agent`
