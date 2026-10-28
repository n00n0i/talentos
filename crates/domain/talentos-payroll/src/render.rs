//! Payslip rendering. Pure: template + data in, HTML out.
//!
//! Tenants control branding and which sections appear — never raw markup. Every
//! interpolated value is escaped, so a company name cannot smuggle a <script>.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// Fields a tenant may hide from the printed payslip.
pub const OPTIONAL_FIELDS: &[&str] = &["ot", "sso", "pf", "tax", "anomalies", "bank"];

/// Tenant-editable payslip configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayslipTemplate {
    pub company_display_name: Option<String>,
    pub logo_url: Option<String>,
    pub accent_color: String,
    pub footer_text: Option<String>,
    pub show_fields: Vec<String>,
    pub paper_size: String,
}

impl Default for PayslipTemplate {
    fn default() -> Self {
        Self {
            company_display_name: None,
            logo_url: None,
            accent_color: "#0369a1".to_string(),
            footer_text: None,
            show_fields: OPTIONAL_FIELDS.iter().map(|s| s.to_string()).collect(),
            paper_size: "A4".to_string(),
        }
    }
}

/// Everything needed to print one payslip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayslipRenderData {
    pub company_name: String,
    pub period_label: String,
    pub payment_date: String,
    pub employee_name: String,
    pub employee_code: String,
    pub position_title: String,
    pub department: Option<String>,
    pub base_salary: Decimal,
    pub ot_amount: Decimal,
    pub gross_earnings: Decimal,
    pub sso_employee: Decimal,
    pub tax_withheld: Decimal,
    pub pf_employee: Decimal,
    pub loan_deduction: Decimal,
    pub total_deductions: Decimal,
    pub net_pay: Decimal,
    pub bank_name: Option<String>,
    pub bank_account_masked: Option<String>,
    pub anomalies: Vec<PayslipAnomaly>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayslipAnomaly {
    pub severity: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TemplateError {
    #[error("accent_color must be a hex colour like #0369a1")]
    BadColor,
    #[error("paper_size must be one of A4, A5, Letter")]
    BadPaperSize,
    #[error("unknown field '{0}' (allowed: ot, sso, pf, tax, anomalies, bank)")]
    UnknownField(String),
    #[error("logo_url must start with https:// or http://")]
    BadLogoUrl,
    #[error("footer_text is limited to 500 characters")]
    FooterTooLong,
}

/// Reject anything the renderer cannot safely honour.
pub fn validate_template(t: &PayslipTemplate) -> Result<(), TemplateError> {
    let c = &t.accent_color;
    let hex_ok =
        c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
    if !hex_ok {
        return Err(TemplateError::BadColor);
    }
    if !matches!(t.paper_size.as_str(), "A4" | "A5" | "Letter") {
        return Err(TemplateError::BadPaperSize);
    }
    for f in &t.show_fields {
        if !OPTIONAL_FIELDS.contains(&f.as_str()) {
            return Err(TemplateError::UnknownField(f.clone()));
        }
    }
    if let Some(url) = &t.logo_url {
        if !url.is_empty() && !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(TemplateError::BadLogoUrl);
        }
    }
    if t.footer_text.as_ref().map_or(0, |f| f.chars().count()) > 500 {
        return Err(TemplateError::FooterTooLong);
    }
    Ok(())
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Thousands-separated, two decimals — Thai payslips group every three digits.
fn baht(v: Decimal) -> String {
    let rounded = v.round_dp(2).to_string();
    let (sign, body) = match rounded.strip_prefix('-') {
        Some(rest) => ("-", rest.to_string()),
        None => ("", rounded),
    };
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (body, "00".to_string()),
    };
    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    for (idx, ch) in int_part.chars().enumerate() {
        if idx > 0 && (int_part.len() - idx) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}{grouped}.{frac_part:0<2}")
}

/// Render a self-contained, printable payslip.
pub fn render_html(t: &PayslipTemplate, d: &PayslipRenderData) -> String {
    let show = |f: &str| t.show_fields.iter().any(|x| x == f);
    let paper = match t.paper_size.as_str() {
        "A5" => "A5",
        "Letter" => "Letter",
        _ => "A4",
    };
    let company = t
        .company_display_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&d.company_name);

    let mut h = String::with_capacity(4096);
    let _ = write!(
        h,
        r#"<!DOCTYPE html>
<html lang="th"><head><meta charset="utf-8">
<title>Payslip {code} {period}</title>
<style>
  @page {{ size: {paper}; margin: 12mm; }}
  * {{ box-sizing: border-box; }}
  body {{ font-family: "Inter","Noto Sans Thai",system-ui,sans-serif; color:#1f2937; margin:0; font-size:12px; }}
  .sheet {{ padding: 8mm; }}
  .top {{ display:flex; justify-content:space-between; align-items:flex-start;
          border-bottom:2px solid {accent}; padding-bottom:8px; }}
  .brand {{ font-size:16px; font-weight:700; color:{accent}; }}
  .muted {{ color:#6b7280; }}
  .title {{ font-size:13px; font-weight:600; text-align:right; }}
  .grid {{ display:grid; grid-template-columns:1fr 1fr; gap:4px 24px; margin:12px 0; }}
  .grid div span {{ color:#6b7280; }}
  table {{ width:100%; border-collapse:collapse; margin-top:8px; }}
  th, td {{ text-align:left; padding:5px 6px; border-bottom:1px solid #e5e7eb; }}
  th {{ background:#f9fafb; font-size:11px; text-transform:uppercase; letter-spacing:.04em; color:#6b7280; }}
  td.num {{ text-align:right; font-variant-numeric: tabular-nums; }}
  .total {{ font-weight:700; border-top:2px solid {accent}; }}
  .warn {{ background:#fffbeb; border-left:3px solid #d97706; padding:6px 8px; margin:4px 0; }}
  .foot {{ margin-top:14px; padding-top:8px; border-top:1px solid #e5e7eb; color:#6b7280; font-size:10px; }}
  img.logo {{ max-height:40px; }}
</style></head><body><div class="sheet">
<div class="top">
  <div>{logo}<div class="brand">{company}</div></div>
  <div class="title">เงินเดือน / PAYSLIP<br><span class="muted">{period}</span></div>
</div>

<div class="grid">
  <div><span>พนักงาน:</span> {emp}</div>
  <div><span>รหัส:</span> {code}</div>
  <div><span>ตำแหน่ง:</span> {position}</div>
  <div><span>แผนก:</span> {dept}</div>
  <div><span>วันจ่าย:</span> {paydate}</div>
  <div>{bank}</div>
</div>

<table>
  <thead><tr><th>รายการ</th><th style="text-align:right">จำนวน (บาท)</th></tr></thead>
  <tbody>
    <tr><td>เงินเดือนฐาน</td><td class="num">{base}</td></tr>
"#,
        paper = paper,
        accent = esc(&t.accent_color),
        logo = t
            .logo_url
            .as_deref()
            .filter(|u| !u.is_empty())
            .map(|u| format!("<img class=\"logo\" src=\"{}\" alt=\"\">", esc(u)))
            .unwrap_or_default(),
        company = esc(company),
        period = esc(&d.period_label),
        emp = esc(&d.employee_name),
        code = esc(&d.employee_code),
        position = esc(&d.position_title),
        dept = esc(d.department.as_deref().unwrap_or("-")),
        paydate = esc(&d.payment_date),
        bank = d
            .bank_account_masked
            .as_deref()
            .filter(|_| show("bank"))
            .map(|acc| format!(
                "<span>บัญชี:</span> {} {}",
                esc(d.bank_name.as_deref().unwrap_or("")),
                esc(acc)
            ))
            .unwrap_or_default(),
        base = baht(d.base_salary),
    );

    if show("ot") && !d.ot_amount.is_zero() {
        let _ = writeln!(
            h,
            "<tr><td>ค่าล่วงเวลา (OT)</td><td class=\"num\">{}</td></tr>",
            baht(d.ot_amount)
        );
    }
    let _ = writeln!(
        h,
        "<tr><td><strong>รายได้รวม</strong></td><td class=\"num\"><strong>{}</strong></td></tr>",
        baht(d.gross_earnings)
    );

    if show("sso") {
        let _ = writeln!(
            h,
            "<tr><td>ประกันสังคม (ลูกจ้าง)</td><td class=\"num\">-{}</td></tr>",
            baht(d.sso_employee)
        );
    }
    if show("tax") {
        let _ = writeln!(
            h,
            "<tr><td>ภาษีหัก ณ ที่จ่าย</td><td class=\"num\">-{}</td></tr>",
            baht(d.tax_withheld)
        );
    }
    if show("pf") {
        let _ = writeln!(
            h,
            "<tr><td>กองทุนสำรองเลี้ยงชีพ</td><td class=\"num\">-{}</td></tr>",
            baht(d.pf_employee)
        );
    }
    if !d.loan_deduction.is_zero() {
        let _ = writeln!(
            h,
            "<tr><td>หักเงินกู้/เบิกล่วงหน้า</td><td class=\"num\">-{}</td></tr>",
            baht(d.loan_deduction)
        );
    }
    let _ = writeln!(
        h,
        "<tr><td>รวมรายการหัก</td><td class=\"num\">-{}</td></tr>",
        baht(d.total_deductions)
    );
    let _ = writeln!(
        h,
        "<tr class=\"total\"><td>เงินได้สุทธิ</td><td class=\"num\">{}</td></tr>",
        baht(d.net_pay)
    );

    h.push_str("</tbody></table>\n");

    if show("anomalies") && !d.anomalies.is_empty() {
        h.push_str("<div style=\"margin-top:10px\"><strong>ข้อสังเกต</strong>\n");
        for a in &d.anomalies {
            let _ = writeln!(
                h,
                "<div class=\"warn\">[{}] {}</div>",
                esc(&a.severity),
                esc(&a.message)
            );
        }
        h.push_str("</div>\n");
    }

    let footer = t
        .footer_text
        .as_deref()
        .unwrap_or("เอกสารนี้สร้างโดยระบบ TalentOS — กรุณาติดต่อฝ่ายบุคคลหากพบข้อผิดพลาด");
    let _ = write!(
        h,
        "<div class=\"foot\">{}</div></div></body></html>",
        esc(footer)
    );
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn data() -> PayslipRenderData {
        PayslipRenderData {
            company_name: "Acme Co".into(),
            period_label: "ตุลาคม 2569".into(),
            payment_date: "2026-11-01".into(),
            employee_name: "สมชาย ใจดี".into(),
            employee_code: "EMP001".into(),
            position_title: "Engineer".into(),
            department: Some("IT".into()),
            base_salary: dec!(85000),
            ot_amount: dec!(1500),
            gross_earnings: dec!(86500),
            sso_employee: dec!(750),
            tax_withheld: dec!(9416.67),
            pf_employee: dec!(4250),
            loan_deduction: dec!(0),
            total_deductions: dec!(14416.67),
            net_pay: dec!(72083.33),
            bank_name: Some("BBL".into()),
            bank_account_masked: Some("****1234".into()),
            anomalies: vec![PayslipAnomaly {
                severity: "warning".into(),
                message: "OT สูง".into(),
            }],
        }
    }

    #[test]
    fn renders_totals_and_period() {
        let html = render_html(&PayslipTemplate::default(), &data());
        assert!(html.contains("ตุลาคม 2569"));
        assert!(
            html.contains("72,083.33"),
            "net pay formatted with separators"
        );
        assert!(html.contains("9,416.67"));
        assert!(html.contains("A4"));
    }

    #[test]
    fn hidden_fields_are_not_rendered() {
        let t = PayslipTemplate {
            show_fields: vec![],
            ..Default::default()
        };
        let html = render_html(&t, &data());
        assert!(!html.contains("ประกันสังคม"));
        assert!(!html.contains("ภาษีหัก"));
        assert!(!html.contains("ข้อสังเกต"));
        // totals still present — the payslip is useless without them
        assert!(html.contains("เงินได้สุทธิ"));
    }

    #[test]
    fn user_content_is_escaped() {
        let mut d = data();
        d.employee_name = "<script>alert(1)</script>".into();
        let t = PayslipTemplate {
            footer_text: Some("<b>x</b>".into()),
            ..Default::default()
        };
        let html = render_html(&t, &d);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<b>x</b>"));
    }

    #[test]
    fn custom_branding_is_used() {
        let t = PayslipTemplate {
            company_display_name: Some("บริษัท ตัวอย่าง จำกัด".into()),
            accent_color: "#b91c1c".into(),
            logo_url: Some("https://example.com/logo.png".into()),
            ..Default::default()
        };
        assert!(validate_template(&t).is_ok());
        let html = render_html(&t, &data());
        assert!(html.contains("บริษัท ตัวอย่าง จำกัด"));
        assert!(html.contains("#b91c1c"));
        assert!(html.contains("https://example.com/logo.png"));
    }

    #[test]
    fn renders_loan_deduction_line_when_present() {
        let mut d = data();
        d.loan_deduction = dec!(1500);
        d.total_deductions += dec!(1500);
        d.net_pay -= dec!(1500);
        let html = render_html(&PayslipTemplate::default(), &d);
        assert!(html.contains("หักเงินกู้/เบิกล่วงหน้า"));
        assert!(html.contains("1,500.00"));
    }

    #[test]
    fn rejects_unsafe_or_unknown_config() {
        let bad_color = PayslipTemplate {
            accent_color: "red;}</style><script>".into(),
            ..Default::default()
        };
        assert_eq!(validate_template(&bad_color), Err(TemplateError::BadColor));

        let bad_field = PayslipTemplate {
            show_fields: vec!["salary".into()],
            ..Default::default()
        };
        assert_eq!(
            validate_template(&bad_field),
            Err(TemplateError::UnknownField("salary".into()))
        );

        let bad_logo = PayslipTemplate {
            logo_url: Some("javascript:alert(1)".into()),
            ..Default::default()
        };
        assert_eq!(validate_template(&bad_logo), Err(TemplateError::BadLogoUrl));

        let bad_paper = PayslipTemplate {
            paper_size: "A3".into(),
            ..Default::default()
        };
        assert_eq!(
            validate_template(&bad_paper),
            Err(TemplateError::BadPaperSize)
        );
    }
}
