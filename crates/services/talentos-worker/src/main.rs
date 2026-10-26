use std::env;
use std::sync::Arc;
use chrono::NaiveDate;
use sqlx::PgPool;
use talentos_kernel::{TenantContext, TenantId, EntityId, UserId, Money};
use talentos_payroll::{PayrollContext, EmployeePayrollInput, PayrollDag, PayrollResult};
use temporal_sdk::{Activity, ActivityContext, Worker};
use temporal_client::Client;

mod workflows;
mod activities;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter("info,temporal=debug")
        .init();

    let db_url = env::var("DATABASE_URL")?;
    let temporal_addr = env::var("TEMPORAL_ADDRESS").unwrap_or_else(|_| "localhost:7233".into());
    let namespace = env::var("TEMPORAL_NAMESPACE").unwrap_or_else(|_| "talentos-dev".into());

    let pool = PgPool::connect(&db_url).await?;
    let client = Client::builder()
        .without_id()
        .build(temporal_addr)?;

    let worker = Worker::new(client, namespace.clone(), "talentos-payroll").await?;

    // Register activities
    activities::register(worker).await?;

    tracing::info!(namespace, "starting payroll worker");
    worker.await
}