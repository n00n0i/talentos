#![allow(clippy::explicit_auto_deref)]
mod common;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::PgPool;
use std::str::FromStr;
use talentos_hr_runner::reports::{
    export_gl_csv, export_pf_remittance_csv, headcount, ot_trend, payroll_cost, ReportError,
};
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
        false,
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
        false,
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
    pf_member: bool,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let employee_id = Uuid::new_v4();
    let mut tx = tenant_tx(pool, tenant_id).await?;
    sqlx::query(
        "INSERT INTO employees \
           (id, tenant_id, entity_id, user_id, employee_code, first_name_th, last_name_th, \
            hire_date, position_title, department, base_salary, tax_filing_status, pf_member, status) \
         VALUES ($1,$2,$3,$4,$5,'ทดสอบ','พนักงาน','2024-01-01','Staff',$6,$7,1,$8,$9)",
    )
    .bind(employee_id)
    .bind(tenant_id)
    .bind(entity_id)
    .bind(user_id)
    .bind(code)
    .bind(department)
    .bind(base_salary)
    .bind(pf_member)
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
        dec!(0),
        dec!(0),
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
        dec!(0),
        dec!(0),
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
        dec!(0),
        dec!(0),
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
    loan_deduction: Decimal,
    reimbursement: Decimal,
) -> Result<(), Box<dyn std::error::Error>> {
    let gross = base_salary + ot;
    sqlx::query(
        "INSERT INTO payslips \
           (id, payroll_run_id, tenant_id, employee_id, base_salary, gross_earnings, \
            total_deductions, net_pay, tax_withheld, sso_employee, sso_employer, \
            pf_employee, pf_employer, loan_deduction, loan_principal, loan_interest, \
            reimbursement, anomalies, status, created_at) \
         VALUES (gen_random_uuid(),$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$13,0,$14,'[]','approved',now())",
    )
    .bind(run_id)
    .bind(tenant_id)
    .bind(employee_id)
    .bind(base_salary)
    .bind(gross)
    .bind(gross - net + reimbursement)
    .bind(net)
    .bind(tax)
    .bind(sso_ee)
    .bind(sso_er)
    .bind(pf_ee)
    .bind(pf_er)
    .bind(loan_deduction)
    .bind(reimbursement)
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
        false,
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
        false,
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

#[tokio::test]
async fn gl_export_balances_with_loan_deduction() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;

    let run_id = Uuid::new_v4();
    let period_start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let period_end = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
    let payment_date = NaiveDate::from_ymd_opt(2026, 4, 5).unwrap();
    let loan = dec!(1000);

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    sqlx::query(
        "INSERT INTO payroll_runs \
           (id, tenant_id, entity_id, period_start, period_end, payment_date, status, \
            total_employees, created_by, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,'approved',1,$7,now())",
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

    // gross 30,000; sso 750; tax 2,000; loan 1,000; net 26,250
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
        dec!(26250),
        loan,
        dec!(0),
    )
    .await?;
    tx.commit().await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let (_, body) = export_gl_csv(&mut *tx, run_id).await?;
    tx.rollback().await?;

    assert!(body.contains("2230"));
    assert!(body.contains("เงินให้กู้พนักงาน"));

    let mut total_debit = dec!(0);
    let mut total_credit = dec!(0);
    for line in body.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
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
async fn gl_export_splits_loan_principal_and_interest() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;

    let run_id = Uuid::new_v4();
    let period_start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let period_end = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
    let payment_date = NaiveDate::from_ymd_opt(2026, 4, 5).unwrap();

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    sqlx::query(
        "INSERT INTO payroll_runs \
           (id, tenant_id, entity_id, period_start, period_end, payment_date, status, \
            total_employees, created_by, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,'approved',1,$7,now())",
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

    // loan_deduction 1,040 = principal 1,000 + interest 40.
    sqlx::query(
        "INSERT INTO payslips \
           (id, payroll_run_id, tenant_id, employee_id, base_salary, gross_earnings, \
            total_deductions, net_pay, tax_withheld, sso_employee, sso_employer, \
            pf_employee, pf_employer, loan_deduction, loan_principal, loan_interest, \
            reimbursement, anomalies, status, created_at) \
         VALUES (gen_random_uuid(),$1,$2,$3,30000,30000,2790,27210,2000,750,750,0,0,1040,1000,40,0,'[]','approved',now())",
    )
    .bind(run_id)
    .bind(seed.tenant_id)
    .bind(seed.emp_active_it)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let (_, body) = export_gl_csv(&mut *tx, run_id).await?;
    tx.rollback().await?;

    assert!(body.contains("2230"));
    assert!(body.contains("เงินให้กู้พนักงาน"));
    assert!(body.contains("4900"));
    assert!(body.contains("รายได้ดอกเบี้ย"));

    // 2230 line credits 1000, 4900 line credits 40.
    let mut principal_credit = dec!(0);
    let mut interest_credit = dec!(0);
    for line in body.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        let code = strip_csv_quotes(cols[0]);
        let credit = Decimal::from_str(strip_csv_quotes(cols[4])).unwrap_or_default();
        if code == "2230" {
            principal_credit += credit;
        }
        if code == "4900" {
            interest_credit += credit;
        }
    }
    assert_eq!(principal_credit, dec!(1000));
    assert_eq!(interest_credit, dec!(40));

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn gl_export_balances_with_reimbursement() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;

    let run_id = Uuid::new_v4();
    let period_start = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
    let period_end = NaiveDate::from_ymd_opt(2026, 4, 30).unwrap();
    let payment_date = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
    let reimbursement = dec!(1500);

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    sqlx::query(
        "INSERT INTO payroll_runs \
           (id, tenant_id, entity_id, period_start, period_end, payment_date, status, \
            total_employees, created_by, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,'approved',1,$7,now())",
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

    // gross 30,000; sso 750; tax 2,000; reimbursement 1,500; net 28,750
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
        dec!(28750),
        dec!(0),
        reimbursement,
    )
    .await?;
    tx.commit().await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let (_, body) = export_gl_csv(&mut *tx, run_id).await?;
    tx.rollback().await?;

    assert!(body.contains("5200"));
    assert!(body.contains("ค่าใช้จ่ายพนักงาน"));

    let mut total_debit = dec!(0);
    let mut total_credit = dec!(0);
    for line in body.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
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
async fn pf_remittance_csv_includes_only_pf_members_and_totals(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;

    let pf_a = seed_employee(
        &pool,
        seed.tenant_id,
        seed.entity_id,
        seed.user_id,
        "E101",
        "IT",
        "active",
        dec!(30000),
        true,
    )
    .await?;
    let pf_b = seed_employee(
        &pool,
        seed.tenant_id,
        seed.entity_id,
        seed.user_id,
        "E102",
        "Sales",
        "active",
        dec!(25000),
        true,
    )
    .await?;

    let run_id = Uuid::new_v4();
    let period_start = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
    let period_end = NaiveDate::from_ymd_opt(2026, 4, 30).unwrap();
    let payment_date = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    sqlx::query(
        "INSERT INTO payroll_runs \
           (id, tenant_id, entity_id, period_start, period_end, payment_date, status, \
            total_employees, created_by, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,'approved',3,$7,now())",
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

    // Non-member: no PF contribution.
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
        dec!(27750),
        dec!(0),
        dec!(0),
    )
    .await?;
    // Member A: employee 1,500 + employer 1,500.
    insert_payslip(
        &mut tx,
        seed.tenant_id,
        run_id,
        pf_a,
        dec!(30000),
        dec!(0),
        dec!(750),
        dec!(750),
        dec!(2000),
        dec!(1500),
        dec!(1500),
        dec!(24750),
        dec!(0),
        dec!(0),
    )
    .await?;
    // Member B: employee 1,250 + employer 1,250.
    insert_payslip(
        &mut tx,
        seed.tenant_id,
        run_id,
        pf_b,
        dec!(25000),
        dec!(0),
        dec!(625),
        dec!(625),
        dec!(1500),
        dec!(1250),
        dec!(1250),
        dec!(21000),
        dec!(0),
        dec!(0),
    )
    .await?;
    tx.commit().await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let (filename, body1) = export_pf_remittance_csv(&mut *tx, run_id).await?;
    let (_, body2) = export_pf_remittance_csv(&mut *tx, run_id).await?;
    tx.rollback().await?;

    assert!(filename.starts_with("pf-remittance-"));
    assert_eq!(body1, body2);

    let lines: Vec<&str> = body1.lines().collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(
        lines[0],
        "\u{feff}employee_code,name,employee_amount,employer_amount,total"
    );
    assert!(lines[1].starts_with("\"E101\""));
    assert!(lines[2].starts_with("\"E102\""));

    let totals: Vec<&str> = lines[3].split(',').collect();
    assert_eq!(totals.len(), 5);
    assert_eq!(strip_csv_quotes(totals[2]), "2750.00");
    assert_eq!(strip_csv_quotes(totals[3]), "2750.00");
    assert_eq!(strip_csv_quotes(totals[4]), "5500.00");

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn pf_remittance_csv_rejects_draft_run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;
    let run_id = seed_draft_payroll_run(&pool, &seed).await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let result = export_pf_remittance_csv(&mut *tx, run_id).await;
    tx.rollback().await?;

    assert!(matches!(result, Err(ReportError::NotFound)));

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn gl_export_uses_custom_account_codes() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    sqlx::query(
        "INSERT INTO tenant_payroll_settings \
           (tenant_id, unpaid_leave_divisor, ot_weekday_rate, ot_weekend_rate, ot_holiday_rate, \
            gl_salary_account, gl_net_account, gl_tax_account, gl_sso_account, gl_pf_account, \
            gl_loan_account, gl_claim_account) \
         VALUES ($1, 30, 1.50, 2.00, 3.00, '6100','2100','2200','2210','2220','2230','6101')",
    )
    .bind(seed.tenant_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let run_id = seed_approved_payroll_run(&pool, &seed).await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let (_, body) = export_gl_csv(&mut *tx, run_id).await?;
    tx.rollback().await?;

    assert!(body.contains("\"6100\""), "custom salary account code");
    assert!(body.contains("\"6101\""), "custom claim account code");
    assert!(body.contains("ค่าแรง"), "Thai account name is preserved");
    assert!(
        !body.contains("\"5100\""),
        "default salary code is replaced"
    );

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn gl_export_groups_by_cost_center_with_department_fallback(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;

    // IT employee gets a cost center; Sales employee stays NULL → department fallback.
    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    sqlx::query("UPDATE employees SET cost_center = $1 WHERE id = $2")
        .bind("CC-100")
        .bind(seed.emp_active_it)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let run_id = seed_approved_payroll_run(&pool, &seed).await?;

    let mut tx = tenant_tx(&pool, seed.tenant_id).await?;
    let (_, body) = export_gl_csv(&mut *tx, run_id).await?;
    tx.rollback().await?;

    assert!(body.contains("CC-100"), "grouped by cost center");
    assert!(body.contains("Sales"), "falls back to department");
    assert!(!body.contains("ไอที"), "department replaced by cost center");

    cleanup_tenant(&pool, seed.tenant_id).await;
    Ok(())
}

fn strip_csv_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(value)
}
