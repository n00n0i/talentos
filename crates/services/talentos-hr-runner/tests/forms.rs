#![allow(clippy::explicit_auto_deref)]
mod common;

use sqlx::PgPool;
use talentos_hr_runner::forms::{
    export_sso1_10_csv, load_active_template, render_50tawi, save_template, FormConfig, FormError,
    FORM_TYPE_50TAWI, FORM_TYPE_SSO1_10,
};
use uuid::Uuid;

struct Seed {
    tenant_id: Uuid,
    entity_id: Uuid,
    user_id: Uuid,
    employee_id: Uuid,
}

async fn tenant_conn(
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

async fn seed_tenant(pool: &PgPool) -> Result<Seed, Box<dyn std::error::Error>> {
    let tenant_id = Uuid::new_v4();
    let entity_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let employee_id = Uuid::new_v4();

    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.current_tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;

    sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'Form Test', $2)")
        .bind(tenant_id)
        .bind(format!("ft-{}", &tenant_id.to_string()[..8]))
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO entities (id, tenant_id, name, tax_id, address) \
         VALUES ($1, $2, 'Form Co', '0105544000001', '{\"line1\":\"1 Road\",\"province\":\"Bangkok\"}')",
    )
    .bind(entity_id)
    .bind(tenant_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO users (id, tenant_id, email, password_hash, is_active) VALUES ($1,$2,$3,'x',true)",
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(format!("u-{}@example.com", &user_id.to_string()[..8]))
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO employees \
           (id, tenant_id, entity_id, user_id, employee_code, first_name_th, last_name_th, \
            national_id, hire_date, position_title, base_salary, tax_filing_status, pf_member, status) \
         VALUES ($1,$2,$3,$4,'E001','สมชาย','ทดสอบ','1234567890123','2024-01-01','Engineer',30000,1,false,'active')",
    )
    .bind(employee_id)
    .bind(tenant_id)
    .bind(entity_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Seed {
        tenant_id,
        entity_id,
        user_id,
        employee_id,
    })
}

async fn seed_run(
    pool: &PgPool,
    seed: &Seed,
    status: &str,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let run_id = Uuid::new_v4();
    let mut tx = tenant_conn(pool, seed.tenant_id).await?;
    sqlx::query(
        "INSERT INTO payroll_runs \
           (id, tenant_id, entity_id, period_start, period_end, payment_date, status, total_employees, created_by) \
         VALUES ($1,$2,$3,'2026-01-01','2026-01-31','2026-02-05',$4,1,$5)",
    )
    .bind(run_id)
    .bind(seed.tenant_id)
    .bind(seed.entity_id)
    .bind(status)
    .bind(seed.user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO payslips \
           (id, payroll_run_id, tenant_id, employee_id, base_salary, gross_earnings, \
            total_deductions, net_pay, tax_withheld, sso_employee, sso_employer, \
            pf_employee, pf_employer, anomalies, status) \
         VALUES (gen_random_uuid(),$1,$2,$3,30000,30000,2250,27750,2000,750,750,0,0,'[]','approved')",
    )
    .bind(run_id)
    .bind(seed.tenant_id)
    .bind(seed.employee_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(run_id)
}

fn tawi_config(label: &str) -> FormConfig {
    serde_json::from_value(serde_json::json!({
        "title": "หนังสือรับรอง (แก้ไข)",
        "fields": [
            {"key":"payer_name","label":label,"source":"entity_name"},
            {"key":"amount","label":"จำนวนเงิน","source":"gross"},
            {"key":"cert_no","label":"เลขที่","source":"cert_no"},
            {"key":"nid","label":"บัตรประชาชน","source":"national_id"},
        ],
    }))
    .unwrap()
}

fn sso_config(order: &[&str]) -> FormConfig {
    let columns: Vec<serde_json::Value> = order
        .iter()
        .map(|s| {
            let label = match *s {
                "national_id" => "บัตร",
                "employee_name" => "ชื่อ",
                "gross" => "ค่าจ้าง",
                "sso_employee" => "สมทบลูกจ้าง",
                "sso_employer" => "สมทบนายจ้าง",
                _ => *s,
            };
            serde_json::json!({"label": label, "source": s})
        })
        .collect();
    serde_json::from_value(serde_json::json!({
        "title": "สปส.1-10 (แก้ไข)",
        "header": [{"key":"employer_name","label":"นายจ้าง","source":"entity_name"}],
        "columns": columns,
    }))
    .unwrap()
}

#[tokio::test]
async fn tenant_override_and_global_fallback() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;

    // No tenant row → global default, not an override.
    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    let t = load_active_template(&mut *tx, FORM_TYPE_50TAWI).await?;
    assert!(!t.is_tenant_override);
    assert_eq!(t.version, 1);
    assert_eq!(t.config.fields.len(), 10);
    tx.rollback().await?;

    // Save an override; version increments and the edited label is served.
    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    let v1 = save_template(
        &mut *tx,
        FORM_TYPE_50TAWI,
        &tawi_config("นายจ้าง (แก้)"),
        seed.user_id,
    )
    .await?;
    tx.commit().await?;
    assert_eq!(v1, 1);

    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    let t = load_active_template(&mut *tx, FORM_TYPE_50TAWI).await?;
    assert!(t.is_tenant_override);
    assert_eq!(t.config.fields[0].label, "นายจ้าง (แก้)");
    tx.rollback().await?;

    common::cleanup(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn save_increments_version_and_keeps_old() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;

    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    let v1 = save_template(
        &mut *tx,
        FORM_TYPE_50TAWI,
        &tawi_config("หนึ่ง"),
        seed.user_id,
    )
    .await?;
    let v2 = save_template(
        &mut *tx,
        FORM_TYPE_50TAWI,
        &tawi_config("สอง"),
        seed.user_id,
    )
    .await?;
    tx.commit().await?;
    assert_eq!((v1, v2), (1, 2));

    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    let versions: Vec<i32> = sqlx::query_scalar(
        "SELECT version FROM form_templates \
         WHERE tenant_id = current_tenant_id() AND form_type = $1 ORDER BY version",
    )
    .bind(FORM_TYPE_50TAWI)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    assert_eq!(versions, vec![1, 2]);

    common::cleanup(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn render_50tawi_uses_edited_labels_and_values() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;
    let run_id = seed_run(&pool, &seed, "approved").await?;

    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    save_template(
        &mut *tx,
        FORM_TYPE_50TAWI,
        &tawi_config("ผู้จ่ายเงิน (แก้ไข)"),
        seed.user_id,
    )
    .await?;
    tx.commit().await?;

    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    let payslip_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM payslips WHERE payroll_run_id = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(run_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.rollback().await?;

    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    let html = render_50tawi(&mut *tx, payslip_id, &None).await?;
    tx.rollback().await?;

    assert!(html.contains("ผู้จ่ายเงิน (แก้ไข)"), "edited label present");
    assert!(html.contains("Form Co"), "entity name resolved");
    assert!(html.contains("30000.00"), "gross resolved");
    assert!(
        html.contains("1234567890123"),
        "national id decrypted/plaintext"
    );
    assert!(html.contains("50T-"), "certificate number resolved");
    assert!(
        html.contains(&run_id.to_string()[..8]),
        "cert no uses run short id"
    );

    common::cleanup(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn sso_csv_honors_columns_order_and_totals() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;
    let run_id = seed_run(&pool, &seed, "approved").await?;

    // Reorder columns and drop national_id to prove order is honored.
    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    save_template(
        &mut *tx,
        FORM_TYPE_SSO1_10,
        &sso_config(&["gross", "employee_name", "sso_employee", "sso_employer"]),
        seed.user_id,
    )
    .await?;
    tx.commit().await?;

    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    let (filename, body1) = export_sso1_10_csv(&mut *tx, run_id, &None).await?;
    let (_, body2) = export_sso1_10_csv(&mut *tx, run_id, &None).await?;
    tx.rollback().await?;

    assert!(filename.starts_with("sso1-10-"));
    assert_eq!(body1, body2, "deterministic");

    let lines: Vec<&str> = body1.lines().collect();
    // header metadata (1), column header (1), data (1), totals (1).
    assert_eq!(lines.len(), 4);
    assert!(lines[0].contains("นายจ้าง"), "header label");
    assert!(lines[0].contains("Form Co"), "header entity name");
    assert!(lines[1].contains("\"ค่าจ้าง\""), "column label");
    assert!(lines[1].contains("\"ชื่อ\""), "column label");
    // columns in edited order: gross, name, sso_employee, sso_employer.
    assert!(lines[1].starts_with("\"ค่าจ้าง\",\"ชื่อ\""), "column order");
    // data row: gross 30000.00, name, 750.00, 750.00.
    assert!(lines[2].starts_with("\"30000.00\",\"สมชาย ทดสอบ\",\"750.00\",\"750.00\""));
    // totals row: รวม, empty, 750.00, 750.00.
    assert!(lines[3].starts_with("\"รวม 30000.00\",\"\",\"750.00\",\"750.00\""));

    common::cleanup(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn sso_csv_rejects_unapproved_run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let seed = seed_tenant(&pool).await?;
    let run_id = seed_run(&pool, &seed, "draft").await?;

    let mut tx = tenant_conn(&pool, seed.tenant_id).await?;
    let result = export_sso1_10_csv(&mut *tx, run_id, &None).await;
    tx.rollback().await?;
    assert!(matches!(result, Err(FormError::NotFound)));

    common::cleanup(&pool, seed.tenant_id).await;
    Ok(())
}

#[tokio::test]
async fn tenant_template_is_not_visible_to_other_tenant() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(url) = common::database_url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let a = seed_tenant(&pool).await?;
    let b = seed_tenant(&pool).await?;

    let mut tx = tenant_conn(&pool, a.tenant_id).await?;
    save_template(
        &mut *tx,
        FORM_TYPE_50TAWI,
        &tawi_config("เฉพาะ A"),
        a.user_id,
    )
    .await?;
    tx.commit().await?;

    // Tenant B still sees the global default, not A's override.
    let mut tx = tenant_conn(&pool, b.tenant_id).await?;
    let t = load_active_template(&mut *tx, FORM_TYPE_50TAWI).await?;
    tx.rollback().await?;
    assert!(!t.is_tenant_override);
    assert_ne!(t.config.fields[0].label, "เฉพาะ A");

    // B cannot read A's rows directly through the tenant-scoped connection.
    let mut tx = tenant_conn(&pool, b.tenant_id).await?;
    let visible: i64 =
        sqlx::query_scalar("SELECT count(*) FROM form_templates WHERE tenant_id = $1")
            .bind(a.tenant_id)
            .fetch_one(&mut *tx)
            .await?;
    tx.rollback().await?;
    assert_eq!(visible, 0);

    common::cleanup(&pool, a.tenant_id).await;
    common::cleanup(&pool, b.tenant_id).await;
    Ok(())
}
