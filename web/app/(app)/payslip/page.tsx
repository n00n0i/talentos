"use client";

import type { TranslationKey } from "@/lib/i18n-catalog";

import { useI18N, tNow } from "@/lib/i18n";

import { useEffect, useMemo, useState } from "react";
import { gw, gql, handleApiError, PayrollRun, Payslip, thDate } from "@/lib/api";

type Template = {
  company_display_name: string | null;
  logo_url: string | null;
  accent_color: string;
  footer_text: string | null;
  show_fields: string[];
  paper_size: string;
};

type Pick = { label: string; id: string };

const FIELDS: { key: string; labelKey: TranslationKey }[] = [
  { key: "ot", labelKey: "payslip.overtime" },
  { key: "sso", labelKey: "payslip.social-security" },
  { key: "pf", labelKey: "payslip.provident-fund" },
  { key: "tax", labelKey: "payslip.withholding-tax" },
  { key: "anomalies", labelKey: "payslip.unusual-item" },
  { key: "bank", labelKey: "payslip.bank-account" },
];

const EMPTY: Template = {
  company_display_name: "",
  logo_url: "",
  accent_color: "#635bff",
  footer_text: "",
  show_fields: FIELDS.map((f) => f.key),
  paper_size: "A4",
};

export default function PayslipPage() {
  const { t: tr } = useI18N();
  const [template, setTemplate] = useState<Template | null>(null);
  const [picks, setPicks] = useState<Pick[]>([]);
  const [selected, setSelected] = useState("");
  const [version, setVersion] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  useEffect(() => {
    Promise.all([gw<Template>("api/v1/payslip/template"), loadPicks()])
      .then(([t, p]) => {
        setTemplate(t);
        setPicks(p);
        if (p[0]) setSelected(p[0].id);
      })
      .catch((e) => handleApiError(e, setError));
  }, []);

  async function loadPicks(): Promise<Pick[]> {
    const runs = await gql<{ payrollRuns: PayrollRun[] }>(
      "{ payrollRuns(limit: 6) { id periodStart periodEnd paymentDate status totalEmployees totalGross totalNet totalTax totalSso totalPf } }",
    );
    const out: Pick[] = [];
    for (const run of runs.payrollRuns) {
      try {
        const r = await gw<{ payslips: Payslip[] }>(`api/v1/payroll/runs/${run.id}/payslips`);
        for (const p of r.payslips.slice(0, 3)) {
          out.push({ id: p.payslip_id, label: `${thDate(run.periodStart)} · ${p.employee_id.slice(0, 8)}` });
        }
      } catch {
        // run without payslips yet
      }
      if (out.length >= 12) break;
    }
    return out;
  }

  const previewUrl = useMemo(() => {
    if (!selected) return "";
    return `/api/gw/api/v1/payroll/payslips/${selected}/render?v=${version}`;
  }, [selected, version]);

  function update<K extends keyof Template>(key: K, value: Template[K]) {
    setTemplate((t) => (t ? { ...t, [key]: value } : t));
  }

  function toggleField(key: string) {
    if (!template) return;
    const has = template.show_fields.includes(key);
    update("show_fields", has ? template.show_fields.filter((f) => f !== key) : [...template.show_fields, key]);
  }

  async function save(e: React.FormEvent) {
    e.preventDefault();
    if (!template) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const body = {
        ...template,
        company_display_name: template.company_display_name?.trim() || null,
        logo_url: template.logo_url?.trim() || null,
        footer_text: template.footer_text?.trim() || null,
      };
      await gw("api/v1/payslip/template", { method: "PUT", body: JSON.stringify(body) });
      setNotice(tr("payslip.form-saved"));
      setVersion((v) => v + 1);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  if (!template) {
    return <div className="spinner">{error || tr("payslip.loading")}</div>;
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{tr("payslip.payslip-form")}</h1>
          <p className="page-sub">{tr("payslip.adjust-the-slip-header-colors-and")}</p>
        </div>
        {selected && (
          <a className="btn" href={`/api/gw/api/v1/payroll/payslips/${selected}/pdf`} target="_blank">
            {tr("payslip.download-pdf")}
          </a>
        )}
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && <p className="badge success" role="status" aria-live="polite">{notice}</p>}

      <div className="editor">
        <form className="card card-pad" onSubmit={save} style={{ display: "grid", gap: 16 }}>
          <div className="field">
            <label htmlFor="company">{tr("payslip.company-name-on-the-slip")}</label>
            <input
              id="company"
              placeholder={tr("payslip.leave-blank-to-use-the-name")}
              value={template.company_display_name ?? ""}
              onChange={(e) => update("company_display_name", e.target.value)}
            />
          </div>

          <div className="field">
            <label htmlFor="logo">{tr("payslip.logo-url-https")}</label>
            <input
              id="logo"
              placeholder="https://…"
              value={template.logo_url ?? ""}
              onChange={(e) => update("logo_url", e.target.value)}
            />
          </div>

          <div className="field">
            <label htmlFor="color">{tr("payslip.main-color")}</label>
            <div className="color-row">
              <input
                id="color"
                type="color"
                value={/^#[0-9a-fA-F]{6}$/.test(template.accent_color) ? template.accent_color : "#635bff"}
                onChange={(e) => update("accent_color", e.target.value)}
              />
              <input
                aria-label={tr("payslip.color-code")}
                value={template.accent_color}
                onChange={(e) => update("accent_color", e.target.value)}
              />
            </div>
          </div>

          <div className="field">
            <label htmlFor="paper">{tr("payslip.paper-size")}</label>
            <select id="paper" value={template.paper_size} onChange={(e) => update("paper_size", e.target.value)}>
              <option value="A4">A4</option>
              <option value="A5">A5</option>
              <option value="Letter">Letter</option>
            </select>
          </div>

          <div className="field">
            <label>{tr("payslip.displayed-items")}</label>
            <div className="fields-grid">
              {FIELDS.map((f) => (
                <label className="check" key={f.key}>
                  <input
                    type="checkbox"
                    checked={template.show_fields.includes(f.key)}
                    onChange={() => toggleField(f.key)}
                  />
                  {tr(f.labelKey)}
                </label>
              ))}
            </div>
          </div>

          <div className="field">
            <label htmlFor="footer">{tr("payslip.message-at-the-end-of-the")}</label>
            <textarea
              id="footer"
              maxLength={500}
              value={template.footer_text ?? ""}
              onChange={(e) => update("footer_text", e.target.value)}
            />
            <span className="hint">{tr("payslip.max-500-characters")}</span>
          </div>

          <button className="btn btn-primary" type="submit" disabled={busy}>
            {busy ? tr("payslip.saving") : tr("payslip.save-the-form")}
          </button>
        </form>

        <div style={{ display: "grid", gap: 10 }}>
          <div className="field">
            <label htmlFor="payslip">{tr("payslip.example-from-the-slip")}</label>
            <select id="payslip" value={selected} onChange={(e) => setSelected(e.target.value)}>
              {picks.length === 0 && <option value="">{tr("payslip.there-is-no-slip-in-the")}</option>}
              {picks.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          </div>
          {previewUrl ? (
            <iframe key={previewUrl} className="preview-frame" src={previewUrl} title={tr("payslip.sample-slip")} />
          ) : (
            <div className="card empty">{tr("payslip.create-a-salary-cycle-first-to")}</div>
          )}
        </div>
      </div>
    </>
  );
}
