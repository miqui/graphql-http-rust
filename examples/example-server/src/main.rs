//! Binary entry point for the example server.
//!
//! The actual router/handlers live in `lib.rs` so that both this binary and
//! the wire-level integration tests in `tests/wire.rs` (which bind the
//! router to a real ephemeral TCP port and drive it with `reqwest`) can
//! reuse the exact same `build_router()`.

use example_server::build_router;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let app = build_router();
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("example-server listening on http://{addr}/graphql");
    axum::serve(listener, app).await.unwrap();
}
