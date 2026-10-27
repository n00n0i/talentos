//! Payroll throughput check. Not part of the normal suite:
//!
//! ```bash
//! LOAD_EMPLOYEES=500 cargo test -p talentos-payroll-runner --test load -- --ignored --nocapture
//! ```
mod common;

use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "load test; run explicitly with LOAD_EMPLOYEES and --ignored"]
async fn payroll_handles_many_employees() {
    let Some(url) = common::database_url() else {
        eprintln!("SKIP: DATABASE_URL unset");
        return;
    };
    let n: usize = std::env::var("LOAD_EMPLOYEES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300)
        .clamp(1, 5_000);

    let raw = sqlx::PgPool::connect(&url).await.expect("connect");
    let pool = TenantPool::connect(&url, 4).await.expect("tenant pool");

    let codes: Vec<String> = (0..n).map(|i| format!("LOAD{i:05}")).collect();
    let employees: Vec<(&str, Decimal)> = codes
        .iter()
        .map(|code| (code.as_str(), Decimal::new(40_000, 0)))
        .collect();
    let (tenant_id, _entity, run_id, ids) = common::seed(&raw, &employees).await;
    assert_eq!(ids.len(), n);

    let started = Instant::now();
    let summary = talentos_payroll_runner::execute_run(&pool, TenantId(tenant_id), run_id)
        .await
        .expect("run");
    let elapsed = started.elapsed();

    println!(
        "payroll throughput: {n} employees in {elapsed:?} ({:.1} employees/s)",
        n as f64 / elapsed.as_secs_f64().max(0.001)
    );
    assert_eq!(summary.employees as usize, n);
    assert_eq!(summary.failed, 0);
    let budget = Duration::from_secs(10) + Duration::from_millis(10 * n as u64);
    assert!(
        elapsed < budget,
        "payroll took {elapsed:?}, budget {budget:?}"
    );

    common::cleanup(&raw, tenant_id).await;
}
