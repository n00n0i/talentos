//! Gateway library surface, so integration tests can build the router.

pub mod cli;
pub mod dto;
pub mod handlers;
pub mod metrics;
pub mod metrics_alert;
pub mod middleware;
pub mod notify_worker;
pub mod ratelimit;
pub mod router;
pub mod secrets;
pub mod state;
