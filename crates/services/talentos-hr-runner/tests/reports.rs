#![allow(clippy::explicit_auto_deref)]
mod common;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::PgPool;
use std::str::FromStr;
use talentos_hr_runner::reports::{export_gl_csv, headcount, ot_trend, payroll_cost, ReportError};
use uuid::Uuid;

async fn tenant_tx(
    pool: &PgPool,
    tenant_id: Uuid,
) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, Box<dyn std::error::Error>> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

#[derive(Debug)]
struct Seed {
    tenant_id: Uuid,
    entity_id: Uuid,
    user_id: Uuid,
    emp_active_it: Uuid,
    emp_probation_sales: Uuid,
}

async fn seed_tenant(pool: &PgPool) -> Result<Seed, Box<dyn std::error::Error>> {
    let tenant_id = Uuid::new_v4();
    let entity_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;

    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'Report Test', $2)")
        .bind(tenant_id)
        .bind(format!("rt-{}", &tenant_id.to_string()[..8]))
        .execute(&mut *tx)
        .await?;

    sqlx::query("INSERT INTO entities (id, tenant_id, name) VALUES ($1, $2, 'Main')")
        .bind(entity_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "INSERT INTO users (id, tenant_id, email, password_hash, is_active) \
         VALUES ($1, $2, $3, 'x', true)",
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(format!("u-{}@example.com", &user_id.to_string()[..8]))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let emp_active_it = seed_employee(
        pool,
        tenant_id,
        entity_id,
        user_id,
        "E001",
        "ไอที",
        "active",
        dec!(30000),
    )
    .await?;
    let emp_probation_sales = seed_employee(
        pool,
        tenant_id,
        entity_id,
        user_id,
        "E002",
        "Sales",
        "probation",
        dec!(25000),
    )
    .await?;

    Ok(Seed {
        tenant_id,
        entity_id,
        user_id,
        emp_active_it,
        emp_probation_sales,
    })
}

#[allow(clippy::too_many_arguments)]
async fn seed_employee(
    pool: &PgPool,
    tenant_id: Uuid,
    entity_id: Uuid,
    user_id: Uuid,
    code: &str,
    department: &str,
    status: &str,
    base_salary: Decimal,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let employee_id = Uuid::new_v4();
    let mut tx = tenant_tx(pool, tenant_id).await?;
    sqlx::query(
        "INSERT INTO employees \
           (id, tenant_id, entity_id, user_id, employee_code, first_name_th, last_name_th, \
            hire_date, position_title, department, base_salary, tax_filing_status, pf_member, status) \
         VALUES ($1,$2,$3,$4,$5,'ทดสอบ','พนักงาน','2024-01-01','Staff',$6,$7,1,false,$8)",
    )
    .bind(employee_id)
    .bind(tenant_id)
    .bind(entity_id)
    .bind(user_id)
    .bind(code)
    .bind(department)
    .bind(base_salary)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(employee_id)
}

async fn terminate_employee(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    termination_date: NaiveDate,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tx = tenant_tx(pool, tenant_id).await?;
    sqlx::query(
        "UPDATE employees \
         SET status = 'terminated', termination_date = $1 \
         WHERE id = $2",
    )
    .bind(termination_date)
    .bind(employee_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn seed_approved_payroll_run(
    pool: &PgPool,
    seed: &Seed,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let run_id = Uuid::new_v4();
    let period_start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let period_end = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
    let payment_date = NaiveDate::from_ymd_opt(2026, 2, 5).unwrap();

    let mut tx = tenant_tx(pool, seed.tenant_id).await?;
    sqlx::query(
        "INSERT INTO payroll_runs \
           (id, tenant_id, entity_id, period_start, period_end, payment_date, status, \
            total_employees, created_by, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,'approved',2,$7,now())",
    )
    .bind(run_id)
    .bind(seed.tenant_id)
    .bind(seed.entity_id)
    .bind(period_start)
    .bind(period_end)
    .bind(payment_date)
    .bind(seed.user_id)
    .execute(&mut *tx)
    .await?;

    insert_payslip(
        &mut tx,
        seed.tenant_id,
        run_id,
        seed.emp_active_it,
        dec!(30000),
        dec!(500),
        dec!(750),
        dec!(750),
        dec!(2000),
        dec!(0),
        dec!(0),
        dec!(27750),
    )
    .await?;
    insert_payslip(
        &mut tx,
        seed.tenant_id,
        run_id,
        seed.emp_probation_sales,
        dec!(25000),
        dec!(0),
        dec!(625),
        dec!(625),
        dec!(1500),
        dec!(0),
        dec!(0),
        dec!(22875),
    )
    .await?;

    tx.commit().await?;
    Ok(run_id)
}

async fn seed_draft_payroll_run(
    pool: &PgPool,
    seed: &Seed,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let run_id = Uuid::new_v4();
    let period_start = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
    let period_end = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();
    let payment_date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();

    let mut tx = tenant_tx(pool, seed.tenant_id).await?;
    sqlx::query(
        "INSERT INTO payroll_runs \
           (id, tenant_id, entity_id, period_start, period_end, payment_date, status, \
            total_employees, created_by, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,'draft',1,$7,now())",
    )
    .bind(run_id)
    .bind(seed.tenant_id)
    .bind(seed.entity_id)
    .bind(period_start)
    .bind(period_end)
    .bind(payment_date)
    .bind(seed.user_id)
    .execute(&mut *tx)
    .await?;

    insert_payslip(
        &mut tx,
        seed.tenant_id,
        run_id,
        seed.emp_active_it,
        dec!(30000),
        dec!(0),
        dec!(750),
        dec!(750),
        dec!(2000),
        dec!(0),
        dec!(0),
        dec!(27000),
    )
    .await?;

    tx.commit().await?;
    Ok(run_id)
}

#[allow(clippy::too_many_arguments)]
async fn insert_payslip(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    run_id: Uuid,
    employee_id: Uuid,
    base_salary: Decimal,
    ot: Decimal,
    sso_ee: Decimal,
    sso_er: Decimal,
    tax: Decimal,
    pf_ee: Decimal,
    pf_er: Decimal,
    net: Decimal,
) -> Result<(), Box<dyn std::error::Error>> {
    let gross = base_salary + ot;
    sqlx::query(
        "INSERT INTO payslips \
           (id, payroll_run_id, tenant_id, employee_id, base_salary, gross_earnings, \
            total_deductions, net_pay, tax_withheld, sso_employee, sso_employer, \
            pf_employee, pf_employer, anomalies, status, created_at) \
         VALUES (gen_random_uuid(),$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'[]','approved',now())",
    )
    .bind(run_id)
    .bind(tenant_id)
    .bind(employee_id)
    .bind(base_salary)
    .bind(gross)
    .bind(gross - net)
    .bind(net)
    .bind(tax)
    .bind(sso_ee)
    .bind(sso_er)
    .bind(pf_ee)
    .bind(pf_er)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn seed_attendance_day(
    pool: &PgPool,
    tenant_id: Uuid,
    employee_id: Uuid,
    work_date: NaiveDate,
    ot_minutes: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tx = tenant_tx(pool, tenant_id).await?;
    sqlx::query(
        "INSERT INTO attendance_days \
           (id, tenant_id, employee_id, work_date, worked_minutes, ot_minutes, status) \
         VALUES (gen_random_uuid(),$1,$2,$3,480,$4,'approved')",
    )
    .bind(tenant_id)
    .bind(employee_id)
    .bind(work_date)
    .bind(ot_minutes)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn cleanup_tenant(pool: &PgPool, tenant_id: Uuid) {
    let _ = common::cleanup(pool, tenant_id).await;
}

#[tokio::test]
async fn headcount_counts_active_probation_leave_and_terminated_last_12m(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;

    let _leave_emp = seed_employee(
        &pool,
        seed.tenant_id,
        seed.entity_id,
        seed.user_id,
        "E003",
        "HR",
        "leave",
        dec!(20000),
    )
    .await?;
    let terminated_emp = seed_employee(
        &pool,
        seed.tenant_id,
        seed.entity_id,
        seed.user_id,
        "E004",
        "",
        "active",
        dec!(20000),
    )
    .await?;
    let today = chrono::Utc::now().date_naive();
    terminate_employee(&pool, seed.tenant_id, terminated_emp, today).await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let report = headcount(&mut *tx).await?;
    tx.rollback().await?;

    assert_eq!(report.active, 1);
    assert_eq!(report.probation, 1);
    assert_eq!(report.leave, 1);
    assert_eq!(report.terminated_last_12m, 1);

    let mut departments: Vec<_> = report
        .by_department
        .iter()
        .map(|d| (d.department.clone(), d.count))
        .collect();
    departments.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        departments,
        vec![("HR".into(), 1), ("Sales".into(), 1), ("ไอที".into(), 1),]
    );

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn payroll_cost_groups_approved_runs_by_department() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;
    seed_approved_payroll_run(&pool, &seed).await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let report = payroll_cost(
        &mut *tx,
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
    )
    .await?;
    tx.rollback().await?;

    assert_eq!(report.rows.len(), 2);
    let it = report.rows.iter().find(|r| r.department == "ไอที").unwrap();
    assert_eq!(it.employees, 1);
    assert_eq!(it.gross, dec!(30500));
    assert_eq!(it.net, dec!(27750));
    assert_eq!(it.tax, dec!(2000));
    assert_eq!(it.sso, dec!(1500));
    assert_eq!(it.pf, dec!(0));

    let sales = report
        .rows
        .iter()
        .find(|r| r.department == "Sales")
        .unwrap();
    assert_eq!(sales.gross, dec!(25000));
    assert_eq!(sales.sso, dec!(1250));

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn draft_runs_are_excluded_from_payroll_cost() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;
    seed_draft_payroll_run(&pool, &seed).await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let report = payroll_cost(
        &mut *tx,
        NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
    )
    .await?;
    tx.rollback().await?;

    assert!(report.rows.is_empty());

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn ot_trend_groups_approved_days_by_month() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;

    seed_attendance_day(
        &pool,
        seed.tenant_id,
        seed.emp_active_it,
        NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
        120,
    )
    .await?;
    seed_attendance_day(
        &pool,
        seed.tenant_id,
        seed.emp_active_it,
        NaiveDate::from_ymd_opt(2026, 1, 20).unwrap(),
        60,
    )
    .await?;
    seed_attendance_day(
        &pool,
        seed.tenant_id,
        seed.emp_probation_sales,
        NaiveDate::from_ymd_opt(2026, 2, 5).unwrap(),
        90,
    )
    .await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let report = ot_trend(
        &mut *tx,
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
    )
    .await?;
    tx.rollback().await?;

    assert_eq!(report.rows.len(), 2);
    assert_eq!(report.rows[0].month, "2026-01");
    assert_eq!(report.rows[0].ot_minutes, 180);
    assert_eq!(report.rows[0].approved_days, 2);
    assert_eq!(report.rows[1].month, "2026-02");
    assert_eq!(report.rows[1].ot_minutes, 90);
    assert_eq!(report.rows[1].approved_days, 1);

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn gl_export_balances_and_is_deterministic() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;
    let run_id = seed_approved_payroll_run(&pool, &seed).await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let (filename, body1) = export_gl_csv(&mut *tx, run_id).await?;
    let (_, body2) = export_gl_csv(&mut *tx, run_id).await?;
    tx.rollback().await?;

    assert!(filename.starts_with("gl-journal-"));
    assert_eq!(body1, body2);

    let mut total_debit = dec!(0);
    let mut total_credit = dec!(0);
    for line in body1.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        assert_eq!(cols.len(), 6);
        let debit = Decimal::from_str(strip_csv_quotes(cols[3])).unwrap_or_default();
        let credit = Decimal::from_str(strip_csv_quotes(cols[4])).unwrap_or_default();
        total_debit += debit;
        total_credit += credit;
    }
    assert!((total_debit - total_credit).abs() <= dec!(0.01));

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn gl_export_rejects_draft_run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;
    let run_id = seed_draft_payroll_run(&pool, &seed).await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let result = export_gl_csv(&mut *tx, run_id).await;
    tx.rollback().await?;

    assert!(matches!(result, Err(ReportError::NotFound)));

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn gl_export_is_not_visible_to_other_tenant() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed_a = seed_tenant(&pool).await?;
    let run_id = seed_approved_payroll_run(&pool, &seed_a).await?;

    let seed_b = seed_tenant(&pool).await?;

    let mut tx = tenant_tx(&pool, seed_b.tenant_id).await?;
    let result = export_gl_csv(&mut *tx, run_id).await;
    tx.rollback().await?;

    assert!(matches!(result, Err(ReportError::NotFound)));

    cleanup_tenant(&pool, seed_a.tenant_id).await;
    cleanup_tenant(&pool, seed_b.tenant_id).await;
    Ok(())
}

fn strip_csv_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(value)
}
