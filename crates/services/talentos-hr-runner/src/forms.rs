//! Editable, versioned statutory form templates (50 ทวิ / สปส.1-10) and the
//! printable HTML / CSV renderers that fill them from payroll data.
//!
//! A tenant row overrides the global default (`tenant_id IS NULL`); tenants
//! without a row render the default. Saving always creates a new version and
//! keeps the old ones.

use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Row};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

pub const FORM_TYPE_50TAWI: &str = "50tawi";
pub const FORM_TYPE_SSO1_10: &str = "sso1_10";
pub const FORM_TYPE_PND1: &str = "pnd1";
pub const FORM_TYPE_PND1A: &str = "pnd1a";
pub const FORM_TYPE_BANK_PAYOUT: &str = "bank_payout";

/// Data keys a template field/column may pull from. Anything else is rejected.
pub const ALLOWED_SOURCES: &[&str] = &[
    "entity_name",
    "entity_tax_id",
    "entity_address",
    "sso_account",
    "branch",
    "employee_name",
    "employee_code",
    "national_id",
    "gross",
    "tax",
    "sso_employee",
    "sso_employer",
    "annual_income",
    "annual_tax",
    "payment_date",
    "period_start",
    "period_end",
    "income_type",
    "cert_no",
    "year",
    "bank_name",
    "bank_account",
    "net",
];

/// Sources whose values are summed in the CSV totals row.
const NUMERIC_SOURCES: &[&str] = &[
    "gross",
    "tax",
    "sso_employee",
    "sso_employer",
    "annual_income",
    "annual_tax",
];

/// 1..=40 rows of fields/columns.
const MAX_ROWS: usize = 40;

/// Income type is fixed at 1 (เงินเดือน/ค่าจ้าง) for 50 ทวิ.
const INCOME_TYPE: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormField {
    pub key: String,
    pub label: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeaderField {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormColumn {
    pub label: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FormConfig {
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FormField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header: Vec<HeaderField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<FormColumn>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormTemplate {
    pub form_type: String,
    pub version: i32,
    pub config: FormConfig,
    pub is_tenant_override: bool,
}

#[derive(Debug, Error)]
pub enum FormError {
    #[error("unknown form type")]
    UnknownFormType,
    #[error("invalid template: {0}")]
    Invalid(String),
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

fn is_form_type(form_type: &str) -> bool {
    matches!(
        form_type,
        FORM_TYPE_50TAWI
            | FORM_TYPE_SSO1_10
            | FORM_TYPE_PND1
            | FORM_TYPE_PND1A
            | FORM_TYPE_BANK_PAYOUT
    )
}

fn source_allowed(source: &str) -> bool {
    ALLOWED_SOURCES.contains(&source)
}

/// Validate a config for `form_type`. Enforces non-empty labels and 1..40 rows.
pub fn validate(form_type: &str, config: &FormConfig) -> Result<(), FormError> {
    if !is_form_type(form_type) {
        return Err(FormError::UnknownFormType);
    }
    if config.title.trim().is_empty() {
        return Err(FormError::Invalid("title must not be empty".into()));
    }
    match form_type {
        FORM_TYPE_50TAWI => {
            if config.fields.is_empty() || config.fields.len() > MAX_ROWS {
                return Err(FormError::Invalid(format!(
                    "fields must have 1..{MAX_ROWS} rows"
                )));
            }
            for f in &config.fields {
                if f.label.trim().is_empty() {
                    return Err(FormError::Invalid("field label must not be empty".into()));
                }
                if !source_allowed(&f.source) {
                    return Err(FormError::Invalid(format!("unknown source '{}'", f.source)));
                }
            }
        }
        FORM_TYPE_SSO1_10 | FORM_TYPE_PND1 | FORM_TYPE_PND1A | FORM_TYPE_BANK_PAYOUT => {
            if config.columns.is_empty() || config.columns.len() > MAX_ROWS {
                return Err(FormError::Invalid(format!(
                    "columns must have 1..{MAX_ROWS} rows"
                )));
            }
            for c in &config.columns {
                if c.label.trim().is_empty() {
                    return Err(FormError::Invalid("column label must not be empty".into()));
                }
                if !source_allowed(&c.source) {
                    return Err(FormError::Invalid(format!("unknown source '{}'", c.source)));
                }
            }
            if config.header.len() > MAX_ROWS {
                return Err(FormError::Invalid(format!(
                    "header must have at most {MAX_ROWS} rows"
                )));
            }
            for h in &config.header {
                if h.label.trim().is_empty() {
                    return Err(FormError::Invalid("header label must not be empty".into()));
                }
                let has_source = h.source.as_deref().is_some_and(|s| !s.trim().is_empty());
                let has_value = h.value.as_deref().is_some_and(|v| !v.trim().is_empty());
                if has_source == has_value {
                    return Err(FormError::Invalid(
                        "header row must have exactly one of source or value".into(),
                    ));
                }
                if let Some(source) = h.source.as_deref() {
                    if !source_allowed(source) {
                        return Err(FormError::Invalid(format!("unknown source '{source}'")));
                    }
                }
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

/// Load the active template for the tenant: the tenant's active row if any,
/// else the global default. `NotFound` when neither exists.
pub async fn load_active_template(
    conn: &mut PgConnection,
    form_type: &str,
) -> Result<FormTemplate, FormError> {
    if !is_form_type(form_type) {
        return Err(FormError::UnknownFormType);
    }
    let row = sqlx::query(
        "SELECT tenant_id, version, config FROM form_templates \
         WHERE form_type = $1 AND is_active \
           AND (tenant_id = current_tenant_id() OR tenant_id IS NULL) \
         ORDER BY (tenant_id IS NOT NULL) DESC, version DESC \
         LIMIT 1",
    )
    .bind(form_type)
    .fetch_optional(&mut *conn)
    .await?;

    let Some(row) = row else {
        return Err(FormError::NotFound);
    };
    let tenant_id: Option<Uuid> = row.get("tenant_id");
    let config: FormConfig =
        serde_json::from_value(row.get("config")).map_err(|e| FormError::Invalid(e.to_string()))?;
    Ok(FormTemplate {
        form_type: form_type.to_string(),
        version: row.get("version"),
        config,
        is_tenant_override: tenant_id.is_some(),
    })
}

/// Save a tenant template: validate, deactivate the current tenant version,
/// insert a new one (version 1, or current + 1). Returns the new version.
pub async fn save_template(
    conn: &mut PgConnection,
    form_type: &str,
    config: &FormConfig,
    created_by: Uuid,
) -> Result<i32, FormError> {
    validate(form_type, config)?;

    let current: Option<i32> = sqlx::query_scalar(
        "SELECT version FROM form_templates \
         WHERE form_type = $1 AND tenant_id = current_tenant_id() AND is_active \
         ORDER BY version DESC LIMIT 1",
    )
    .bind(form_type)
    .fetch_optional(&mut *conn)
    .await?;
    let new_version = current.map_or(1, |v| v + 1);

    sqlx::query(
        "UPDATE form_templates SET is_active = FALSE \
         WHERE form_type = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(form_type)
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        "INSERT INTO form_templates (tenant_id, form_type, version, config, is_active, created_by) \
         VALUES (current_tenant_id(), $1, $2, $3, TRUE, $4)",
    )
    .bind(form_type)
    .bind(new_version)
    .bind(serde_json::to_value(config).map_err(|e| FormError::Invalid(e.to_string()))?)
    .bind(created_by)
    .execute(&mut *conn)
    .await?;

    Ok(new_version)
}

/// Decrypt a stored PII value if the cipher is available; an encrypted value
/// without a key is dropped rather than printed.
fn decrypt(
    pii: &Option<Arc<talentos_db::pii::PiiCipher>>,
    stored: Option<String>,
) -> Option<String> {
    let stored = stored?;
    match pii {
        Some(cipher) => cipher.decrypt(&stored).ok(),
        None if talentos_db::pii::is_encrypted(&stored) => None,
        None => Some(stored),
    }
}

/// Human-readable address from the entities JSONB (`{line1, province, ...}`).
fn format_address(address: Option<serde_json::Value>) -> String {
    match address {
        Some(serde_json::Value::Object(map)) => map
            .values()
            .filter_map(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Some(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

fn format_money(value: rust_decimal::Decimal) -> String {
    value.round_dp(2).to_string()
}

fn format_date(value: chrono::NaiveDate) -> String {
    value.format("%d/%m/%Y").to_string()
}

/// Values available when rendering a 50 ทวิ for a single payslip.
struct Tawi50Data {
    entity_name: String,
    entity_tax_id: String,
    entity_address: String,
    employee_name: String,
    employee_code: String,
    national_id: Option<String>,
    gross: String,
    tax: String,
    payment_date: String,
    cert_no: String,
}

impl Tawi50Data {
    fn resolve(&self, source: &str) -> String {
        match source {
            "entity_name" => self.entity_name.clone(),
            "entity_tax_id" => self.entity_tax_id.clone(),
            "entity_address" => self.entity_address.clone(),
            "employee_name" => self.employee_name.clone(),
            "employee_code" => self.employee_code.clone(),
            "national_id" => self.national_id.clone().unwrap_or_default(),
            "gross" => self.gross.clone(),
            "tax" => self.tax.clone(),
            "payment_date" => self.payment_date.clone(),
            "income_type" => INCOME_TYPE.to_string(),
            "cert_no" => self.cert_no.clone(),
            _ => String::new(),
        }
    }
}

/// GET /api/v1/payroll/payslips/:id/50tawi — printable 50 ทวิ HTML using the
/// tenant's active template. Payer comes from the run's entity; national id is
/// decrypted with the app cipher when stored encrypted.
pub async fn render_50tawi(
    conn: &mut PgConnection,
    payslip_id: Uuid,
    pii: &Option<Arc<talentos_db::pii::PiiCipher>>,
) -> Result<String, FormError> {
    let template = load_active_template(conn, FORM_TYPE_50TAWI).await?;

    let row = sqlx::query(
        "SELECT e.first_name_th, e.last_name_th, e.employee_code, e.national_id, \
                p.gross_earnings, p.tax_withheld, \
                r.payment_date, r.id AS run_id, \
                ent.name AS entity_name, ent.tax_id AS entity_tax_id, ent.address AS entity_address \
         FROM payslips p \
         JOIN employees e ON e.id = p.employee_id AND e.tenant_id = p.tenant_id \
         JOIN payroll_runs r ON r.id = p.payroll_run_id \
         JOIN entities ent ON ent.id = r.entity_id \
         WHERE p.id = $1 AND p.tenant_id = current_tenant_id()",
    )
    .bind(payslip_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(FormError::NotFound)?;

    let run_id: Uuid = row.get("run_id");
    let employee_code: String = row.get("employee_code");
    let data = Tawi50Data {
        entity_name: row.get("entity_name"),
        entity_tax_id: row
            .get::<Option<String>, _>("entity_tax_id")
            .unwrap_or_default(),
        entity_address: format_address(row.get("entity_address")),
        employee_name: format!(
            "{} {}",
            row.get::<String, _>("first_name_th"),
            row.get::<String, _>("last_name_th")
        ),
        employee_code: employee_code.clone(),
        national_id: decrypt(pii, row.get("national_id")),
        gross: format_money(row.get("gross_earnings")),
        tax: format_money(row.get("tax_withheld")),
        payment_date: format_date(row.get("payment_date")),
        cert_no: format!("50T-{}-{employee_code}", &run_id.to_string()[..8]),
    };

    let mut rows = String::new();
    for field in &template.config.fields {
        rows.push_str(&format!(
            "<tr><td class=\"lbl\">{}</td><td class=\"val\">{}</td></tr>",
            html_escape(&field.label),
            html_escape(&data.resolve(&field.source)),
        ));
    }

    Ok(format!(
        "<!doctype html><html lang=\"th\"><head><meta charset=\"utf-8\">\
         <title>{title}</title><style>\
         body{{font-family:sans-serif;margin:32px;color:#111}}\
         h1{{font-size:20px;margin-bottom:16px}}\
         table{{border-collapse:collapse;width:100%;max-width:640px}}\
         td{{border:1px solid #ccc;padding:8px 12px}}\
         .lbl{{background:#f5f5f5;width:45%;font-weight:600}}\
         .sig{{margin-top:64px;display:flex;justify-content:space-between}}\
         </style></head><body><h1>{title}</h1><table>{rows}</table>\
         <div class=\"sig\"><div>ลงชื่อ ………………… ผู้จ่ายเงิน</div>\
         <div>ลงชื่อ ………………… ผู้รับเงิน</div></div></body></html>",
        title = html_escape(&template.config.title),
    ))
}

/// Metadata values available for a column-based CSV form's header.
#[derive(Default)]
struct HeaderContext {
    entity_name: String,
    entity_tax_id: String,
    entity_address: String,
    sso_account: String,
    branch: String,
    period_start: String,
    period_end: String,
    year: String,
}

impl HeaderContext {
    fn resolve(&self, source: &str) -> String {
        match source {
            "entity_name" => self.entity_name.clone(),
            "entity_tax_id" => self.entity_tax_id.clone(),
            "entity_address" => self.entity_address.clone(),
            "sso_account" => self.sso_account.clone(),
            "branch" => self.branch.clone(),
            "period_start" => self.period_start.clone(),
            "period_end" => self.period_end.clone(),
            "year" => self.year.clone(),
            _ => String::new(),
        }
    }
}

async fn load_header_context(
    conn: &mut PgConnection,
    entity_id: Uuid,
) -> Result<HeaderContext, FormError> {
    let entity = sqlx::query(
        "SELECT name, tax_id, address, sso_account, branch FROM entities \
         WHERE id = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(entity_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(match entity {
        Some(e) => HeaderContext {
            entity_name: e.get("name"),
            entity_tax_id: e.get::<Option<String>, _>("tax_id").unwrap_or_default(),
            entity_address: e.get::<serde_json::Value, _>("address").to_string(),
            sso_account: e
                .get::<Option<String>, _>("sso_account")
                .unwrap_or_default(),
            branch: e.get("branch"),
            ..Default::default()
        },
        None => HeaderContext::default(),
    })
}

/// Render header rows, the column header, data rows and a totals row for a
/// column-based CSV form.
fn render_columns_csv(
    config: &FormConfig,
    header: &HeaderContext,
    rows: &[sqlx::postgres::PgRow],
    resolve_row: &dyn Fn(&str, &sqlx::postgres::PgRow) -> String,
) -> String {
    let mut body = String::from("\u{feff}");
    for h in &config.header {
        let value = match h.source.as_deref() {
            Some(src) => header.resolve(src),
            None => h.value.clone().unwrap_or_default(),
        };
        body.push_str(&format!(
            "{},{}\r\n",
            csv_field(&h.label),
            csv_field(&value)
        ));
    }

    body.push_str(
        &config
            .columns
            .iter()
            .map(|c| csv_field(&c.label))
            .collect::<Vec<_>>()
            .join(","),
    );
    body.push_str("\r\n");

    let mut totals = vec![rust_decimal::Decimal::ZERO; config.columns.len()];
    for row in rows {
        let values: Vec<String> = config
            .columns
            .iter()
            .map(|c| resolve_row(&c.source, row))
            .collect();
        for (i, c) in config.columns.iter().enumerate() {
            if NUMERIC_SOURCES.contains(&c.source.as_str()) {
                if let Ok(d) = values[i].parse::<rust_decimal::Decimal>() {
                    totals[i] += d;
                }
            }
        }
        body.push_str(
            &values
                .iter()
                .map(|v| csv_field(v))
                .collect::<Vec<_>>()
                .join(","),
        );
        body.push_str("\r\n");
    }

    // Totals row: first column "รวม", numeric columns summed, others empty.
    let total_values: Vec<String> = config
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i == 0 {
                if NUMERIC_SOURCES.contains(&c.source.as_str()) {
                    format!("รวม {}", format_money(totals[i]))
                } else {
                    "รวม".to_string()
                }
            } else if NUMERIC_SOURCES.contains(&c.source.as_str()) {
                format_money(totals[i])
            } else {
                String::new()
            }
        })
        .collect();
    body.push_str(
        &total_values
            .iter()
            .map(|v| csv_field(v))
            .collect::<Vec<_>>()
            .join(","),
    );
    body.push_str("\r\n");

    body
}

async fn export_approved_run_csv(
    conn: &mut PgConnection,
    form_type: &str,
    prefix: &str,
    run_id: Uuid,
    pii: &Option<Arc<talentos_db::pii::PiiCipher>>,
) -> Result<(String, String), FormError> {
    let template = load_active_template(conn, form_type).await?;

    let run = sqlx::query(
        "SELECT status, period_start, period_end, entity_id \
         FROM payroll_runs WHERE id = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(run_id)
    .fetch_optional(&mut *conn)
    .await?;

    let (period_start, period_end, entity_id) = match run {
        Some(row) => {
            let status: String = row.get("status");
            if status != "approved" {
                return Err(FormError::NotFound);
            }
            (
                row.get::<chrono::NaiveDate, _>("period_start"),
                row.get::<chrono::NaiveDate, _>("period_end"),
                row.get::<Uuid, _>("entity_id"),
            )
        }
        None => return Err(FormError::NotFound),
    };

    let mut header = load_header_context(conn, entity_id).await?;
    header.period_start = format_date(period_start);
    header.period_end = format_date(period_end);

    let rows = sqlx::query(
        "SELECT e.national_id, e.first_name_th, e.last_name_th, e.employee_code, \
                p.gross_earnings, p.tax_withheld, p.sso_employee, p.sso_employer \
         FROM payslips p \
         JOIN employees e ON e.id = p.employee_id AND e.tenant_id = p.tenant_id \
         WHERE p.payroll_run_id = $1 AND p.tenant_id = current_tenant_id() \
         ORDER BY e.employee_code",
    )
    .bind(run_id)
    .fetch_all(&mut *conn)
    .await?;

    let filename = format!("{prefix}-{run_id}-{period_start}-{period_end}.csv");
    let body = render_columns_csv(&template.config, &header, &rows, &|src, row| {
        resolve_column_value(src, row, pii)
    });

    Ok((filename, body))
}

/// GET /api/v1/payroll/runs/:id/sso1-10.csv — สปส.1-10 CSV from the active
/// template. Approved runs only. Returns `(filename, body)`.
pub async fn export_sso1_10_csv(
    conn: &mut PgConnection,
    run_id: Uuid,
    pii: &Option<Arc<talentos_db::pii::PiiCipher>>,
) -> Result<(String, String), FormError> {
    export_approved_run_csv(conn, FORM_TYPE_SSO1_10, "sso1-10", run_id, pii).await
}

/// GET /api/v1/payroll/runs/:id/pnd1.csv — ภ.ง.ด.1 CSV (monthly withholding)
/// from the active template. Approved runs only. Returns `(filename, body)`.
pub async fn export_pnd1_csv(
    conn: &mut PgConnection,
    run_id: Uuid,
    pii: &Option<Arc<talentos_db::pii::PiiCipher>>,
) -> Result<(String, String), FormError> {
    export_approved_run_csv(conn, FORM_TYPE_PND1, "pnd1", run_id, pii).await
}

/// GET /api/v1/reports/pnd1a.csv?year= — ภ.ง.ด.1ก CSV (annual withholding),
/// summing gross/tax per employee across approved runs whose period_end falls
/// in `year`. Returns `(filename, body)`.
pub async fn export_pnd1a_csv(
    conn: &mut PgConnection,
    year: i32,
    pii: &Option<Arc<talentos_db::pii::PiiCipher>>,
) -> Result<(String, String), FormError> {
    let template = load_active_template(conn, FORM_TYPE_PND1A).await?;

    let entity_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM entities WHERE tenant_id = current_tenant_id() AND is_active \
         ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(&mut *conn)
    .await?;
    let mut header = match entity_id {
        Some(id) => load_header_context(conn, id).await?,
        None => HeaderContext::default(),
    };
    header.year = year.to_string();

    let from = chrono::NaiveDate::from_ymd_opt(year, 1, 1)
        .ok_or_else(|| FormError::Invalid("invalid year".into()))?;
    let to = chrono::NaiveDate::from_ymd_opt(year, 12, 31)
        .ok_or_else(|| FormError::Invalid("invalid year".into()))?;

    let rows = sqlx::query(
        "SELECT e.national_id, e.first_name_th, e.last_name_th, e.employee_code, \
                SUM(p.gross_earnings) AS annual_income, SUM(p.tax_withheld) AS annual_tax \
         FROM payslips p \
         JOIN employees e ON e.id = p.employee_id AND e.tenant_id = p.tenant_id \
         JOIN payroll_runs r ON r.id = p.payroll_run_id \
         WHERE p.tenant_id = current_tenant_id() \
           AND r.status = 'approved' \
           AND r.period_end >= $1 AND r.period_end <= $2 \
         GROUP BY e.id, e.employee_code, e.first_name_th, e.last_name_th, e.national_id \
         ORDER BY e.employee_code",
    )
    .bind(from)
    .bind(to)
    .fetch_all(&mut *conn)
    .await?;

    let filename = format!("pnd1a-{year}.csv");
    let body = render_columns_csv(&template.config, &header, &rows, &|src, row| {
        resolve_column_value(src, row, pii)
    });

    Ok((filename, body))
}

/// GET /api/v1/payroll/runs/:id/bank-payout.csv — bank transfer file from the
/// active `bank_payout` template. Approved runs only. Column header + one row
/// per payslip (no totals/metadata, so banks parse it directly). Missing bank
/// account becomes an empty cell. Returns `(filename, body)`.
pub async fn export_bank_payout_csv(
    conn: &mut PgConnection,
    run_id: Uuid,
    pii: &Option<Arc<talentos_db::pii::PiiCipher>>,
) -> Result<(String, String), FormError> {
    let template = load_active_template(conn, FORM_TYPE_BANK_PAYOUT).await?;

    let run = sqlx::query(
        "SELECT status, period_start, period_end FROM payroll_runs \
         WHERE id = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(run_id)
    .fetch_optional(&mut *conn)
    .await?;

    let (period_start, period_end) = match run {
        Some(row) => {
            if row.get::<String, _>("status") != "approved" {
                return Err(FormError::NotFound);
            }
            (
                row.get::<chrono::NaiveDate, _>("period_start"),
                row.get::<chrono::NaiveDate, _>("period_end"),
            )
        }
        None => return Err(FormError::NotFound),
    };

    let rows = sqlx::query(
        "SELECT e.employee_code, e.first_name_th, e.last_name_th, \
                e.bank_name, e.bank_account_number, p.net_pay \
         FROM payslips p \
         JOIN employees e ON e.id = p.employee_id AND e.tenant_id = p.tenant_id \
         WHERE p.payroll_run_id = $1 AND p.tenant_id = current_tenant_id() \
         ORDER BY e.employee_code",
    )
    .bind(run_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut body = String::from("\u{feff}");
    body.push_str(
        &template
            .config
            .columns
            .iter()
            .map(|c| csv_field(&c.label))
            .collect::<Vec<_>>()
            .join(","),
    );
    body.push_str("\r\n");
    for row in &rows {
        let values: Vec<String> = template
            .config
            .columns
            .iter()
            .map(|c| resolve_bank_value(&c.source, row, pii))
            .collect();
        body.push_str(
            &values
                .iter()
                .map(|v| csv_field(v))
                .collect::<Vec<_>>()
                .join(","),
        );
        body.push_str("\r\n");
    }

    let filename = format!("bank-payout-{run_id}-{period_start}-{period_end}.csv");
    Ok((filename, body))
}

fn resolve_bank_value(
    source: &str,
    row: &sqlx::postgres::PgRow,
    pii: &Option<Arc<talentos_db::pii::PiiCipher>>,
) -> String {
    match source {
        "employee_code" => row
            .try_get::<String, _>("employee_code")
            .unwrap_or_default(),
        "employee_name" => format!(
            "{} {}",
            row.try_get::<String, _>("first_name_th")
                .unwrap_or_default(),
            row.try_get::<String, _>("last_name_th").unwrap_or_default()
        ),
        "bank_name" => row
            .try_get::<Option<String>, _>("bank_name")
            .ok()
            .flatten()
            .unwrap_or_default(),
        "bank_account" => decrypt(
            pii,
            row.try_get::<Option<String>, _>("bank_account_number")
                .ok()
                .flatten(),
        )
        .unwrap_or_default(),
        "net" => row
            .try_get::<rust_decimal::Decimal, _>("net_pay")
            .map(format_money)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn resolve_column_value(
    source: &str,
    row: &sqlx::postgres::PgRow,
    pii: &Option<Arc<talentos_db::pii::PiiCipher>>,
) -> String {
    match source {
        "national_id" => decrypt(
            pii,
            row.try_get::<Option<String>, _>("national_id")
                .ok()
                .flatten(),
        )
        .unwrap_or_default(),
        "employee_name" => format!(
            "{} {}",
            row.try_get::<String, _>("first_name_th")
                .unwrap_or_default(),
            row.try_get::<String, _>("last_name_th").unwrap_or_default()
        ),
        "employee_code" => row
            .try_get::<String, _>("employee_code")
            .unwrap_or_default(),
        "gross" => row
            .try_get::<rust_decimal::Decimal, _>("gross_earnings")
            .map(format_money)
            .unwrap_or_default(),
        "tax" => row
            .try_get::<rust_decimal::Decimal, _>("tax_withheld")
            .map(format_money)
            .unwrap_or_default(),
        "sso_employee" => row
            .try_get::<rust_decimal::Decimal, _>("sso_employee")
            .map(format_money)
            .unwrap_or_default(),
        "sso_employer" => row
            .try_get::<rust_decimal::Decimal, _>("sso_employer")
            .map(format_money)
            .unwrap_or_default(),
        "annual_income" => row
            .try_get::<rust_decimal::Decimal, _>("annual_income")
            .map(format_money)
            .unwrap_or_default(),
        "annual_tax" => row
            .try_get::<rust_decimal::Decimal, _>("annual_tax")
            .map(format_money)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn csv_field(value: &str) -> String {
    // Spreadsheet formula-injection guard.
    let safe = if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tawi_config() -> FormConfig {
        serde_json::from_value(serde_json::json!({
            "title": "T",
            "fields": [{"key":"a","label":"A","source":"gross"}],
        }))
        .unwrap()
    }

    fn sso_config() -> FormConfig {
        serde_json::from_value(serde_json::json!({
            "title": "S",
            "header": [{"key":"n","label":"N","source":"entity_name"}],
            "columns": [{"label":"L","source":"gross"}],
        }))
        .unwrap()
    }

    #[test]
    fn validates_unknown_source() {
        let mut c = tawi_config();
        c.fields[0].source = "nope".into();
        assert!(matches!(
            validate(FORM_TYPE_50TAWI, &c),
            Err(FormError::Invalid(_))
        ));
    }

    #[test]
    fn validates_empty_label() {
        let mut c = tawi_config();
        c.fields[0].label = "  ".into();
        assert!(matches!(
            validate(FORM_TYPE_50TAWI, &c),
            Err(FormError::Invalid(_))
        ));
    }

    #[test]
    fn validates_row_bounds() {
        let mut c = tawi_config();
        c.fields.clear();
        assert!(matches!(
            validate(FORM_TYPE_50TAWI, &c),
            Err(FormError::Invalid(_))
        ));
    }

    #[test]
    fn validates_header_source_or_value() {
        let mut c = sso_config();
        c.header[0].source = None;
        assert!(matches!(
            validate(FORM_TYPE_SSO1_10, &c),
            Err(FormError::Invalid(_))
        ));
        c.header[0].value = Some("x".into());
        assert!(validate(FORM_TYPE_SSO1_10, &c).is_ok());
    }

    #[test]
    fn html_and_csv_escape() {
        assert_eq!(html_escape("<a&b>"), "&lt;a&amp;b&gt;");
        assert_eq!(csv_field("=cmd"), "\"'=cmd\"");
        assert_eq!(csv_field("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn validates_pnd_column_forms() {
        let c = sso_config();
        assert!(validate(FORM_TYPE_PND1, &c).is_ok());
        assert!(validate(FORM_TYPE_PND1A, &c).is_ok());

        let mut empty = sso_config();
        empty.columns.clear();
        assert!(matches!(
            validate(FORM_TYPE_PND1, &empty),
            Err(FormError::Invalid(_))
        ));
        assert!(matches!(
            validate(FORM_TYPE_PND1A, &empty),
            Err(FormError::Invalid(_))
        ));
    }

    #[test]
    fn validates_annual_sources() {
        let c: FormConfig = serde_json::from_value(serde_json::json!({
            "title": "P",
            "columns": [{"label":"L","source":"annual_income"}],
        }))
        .unwrap();
        assert!(validate(FORM_TYPE_PND1A, &c).is_ok());

        let mut bad = c;
        bad.columns[0].source = "nope".into();
        assert!(matches!(
            validate(FORM_TYPE_PND1A, &bad),
            Err(FormError::Invalid(_))
        ));
    }
}
