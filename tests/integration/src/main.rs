//! Integration test binary entry point.
//!
//! This file is only needed when running with `harness = false`.
//! With standard test harness (default), tests are auto-discovered from
//! modules annotated with `#[tokio::test]`.
//!
//! Run with: `cargo nextest run --test integration --features integration`

fn main() {
    // This is a no-op when using standard test harness.
    // Tests are auto-discovered by cargo test/nextest.
    println!("Integration tests - run with: cargo nextest run --test integration --features integration");
}