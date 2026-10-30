//! Payroll anomaly review: review state cleared on recalculation, and anomaly
//! rows are recorded per payslip. Requires DATABASE_URL; skips when unset.

use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

mod common;

use common::{cleanup, database_url, seed};

/// A recalculation must clear any prior anomaly review so the approver has to
/// re-review the fresh anomalies before approving again.
#[tokio::test]
async fn recalc_clears_anomaly_review() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let admin_url = std::env::var("DATABASE_ADMIN_URL").unwrap_or_else(|_| url.clone());
    let raw = PgPool::connect(&admin_url).await.expect("connect admin");
    let pool = TenantPool::connect(&url, 4).await.expect("tenant pool");

    let (tenant_id, _entity_id, run_id, _employees) =
        seed(&raw, &[("EMP100", Decimal::new(85_000, 0))]).await;

    talentos_payroll_runner::execute_run(&pool, TenantId(tenant_id), run_id)
        .await
        .expect("first run");

    // Simulate an HR review.
    let reviewer: Uuid = sqlx::query_scalar("SELECT created_by FROM payroll_runs WHERE id = $1")
        .bind(run_id)
        .fetch_one(&raw)
        .await
        .expect("created_by");
    sqlx::query(
        "UPDATE payroll_runs SET anomaly_reviewed_at = now(), anomaly_reviewed_by = $2 WHERE id = $1",
    )
    .bind(run_id)
    .bind(reviewer)
    .execute(&raw)
    .await
    .expect("mark reviewed");
    let reviewed: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT anomaly_reviewed_at FROM payroll_runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&raw)
            .await
            .expect("reviewed_at");
    assert!(reviewed.is_some(), "review should be recorded");

    // Recalculate: reset to draft and re-run, which must clear the review.
    sqlx::query("UPDATE payroll_runs SET status = 'draft' WHERE id = $1")
        .bind(run_id)
        .execute(&raw)
        .await
        .expect("reset to draft");
    talentos_payroll_runner::execute_run(&pool, TenantId(tenant_id), run_id)
        .await
        .expect("recalc");

    let row = sqlx::query(
        "SELECT anomaly_reviewed_at, anomaly_reviewed_by FROM payroll_runs WHERE id = $1",
    )
    .bind(run_id)
    .fetch_one(&raw)
    .await
    .expect("review state");
    let reviewed_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get("anomaly_reviewed_at").unwrap_or(None);
    let reviewed_by: Option<Uuid> = row.try_get("anomaly_reviewed_by").unwrap_or(None);
    assert!(
        reviewed_at.is_none(),
        "recalc must clear anomaly_reviewed_at"
    );
    assert!(
        reviewed_by.is_none(),
        "recalc must clear anomaly_reviewed_by"
    );

    cleanup(&raw, tenant_id).await;
}

/// A high salary yields an `sso_cap` warning on the payslip, which is what the
/// gateway flattens into review rows and counts.
#[tokio::test]
async fn high_salary_records_sso_cap_warning() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let admin_url = std::env::var("DATABASE_ADMIN_URL").unwrap_or_else(|_| url.clone());
    let raw = PgPool::connect(&admin_url).await.expect("connect admin");
    let pool = TenantPool::connect(&url, 4).await.expect("tenant pool");

    let (tenant_id, _entity_id, run_id, _employees) =
        seed(&raw, &[("EMP200", Decimal::new(85_000, 0))]).await;
    talentos_payroll_runner::execute_run(&pool, TenantId(tenant_id), run_id)
        .await
        .expect("run");

    let anomalies: serde_json::Value =
        sqlx::query_scalar("SELECT p.anomalies FROM payslips p WHERE p.payroll_run_id = $1")
            .bind(run_id)
            .fetch_one(&raw)
            .await
            .expect("anomalies");

    let arr = anomalies.as_array().expect("anomalies array");
    assert_eq!(arr.len(), 1, "one warning expected");
    assert_eq!(arr[0]["code"], "sso_cap");
    assert_eq!(arr[0]["severity"], "warning");

    cleanup(&raw, tenant_id).await;
}

/// Each tenant only sees its own run's anomalies (tenant scoping at the query level).
#[tokio::test]
async fn anomalies_are_tenant_scoped() {
    let Some(url) = database_url() else {
        eprintln!("SKIP: DATABASE_URL not set");
        return;
    };
    let admin_url = std::env::var("DATABASE_ADMIN_URL").unwrap_or_else(|_| url.clone());
    let raw = PgPool::connect(&admin_url).await.expect("connect admin");
    let pool = TenantPool::connect(&url, 4).await.expect("tenant pool");

    let (ta, _ea, run_a, _emp_a) = seed(&raw, &[("EMPA01", Decimal::new(85_000, 0))]).await;
    let (tb, _eb, run_b, _emp_b) = seed(&raw, &[("EMPB01", Decimal::new(14_000, 0))]).await;

    talentos_payroll_runner::execute_run(&pool, TenantId(ta), run_a)
        .await
        .expect("run A");
    talentos_payroll_runner::execute_run(&pool, TenantId(tb), run_b)
        .await
        .expect("run B");

    // Tenant A's run has one anomaly (sso_cap); tenant B's run (14k) has none.
    let count_a: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM payslips p WHERE p.payroll_run_id = $1 AND jsonb_array_length(p.anomalies) > 0",
    )
    .bind(run_a)
    .fetch_one(&raw)
    .await
    .expect("count A");
    let count_b: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM payslips p WHERE p.payroll_run_id = $1 AND jsonb_array_length(p.anomalies) > 0",
    )
    .bind(run_b)
    .fetch_one(&raw)
    .await
    .expect("count B");

    assert_eq!(count_a, 1);
    assert_eq!(count_b, 0);

    cleanup(&raw, ta).await;
    cleanup(&raw, tb).await;
}
