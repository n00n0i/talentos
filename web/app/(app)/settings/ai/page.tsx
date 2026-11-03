"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { gw, handleApiError } from "@/lib/api";
import { useI18N } from "@/lib/i18n";

type Skill = { id: string; enabled: boolean; allowed_tools?: string[] };

type AiSettings = {
  provider: string;
  base_url: string | null;
  model: string;
  mcp_enabled: boolean;
  skills: unknown;
  api_key_set: boolean;
  secrets_backend: "openbao" | "none";
};

type TestResult = { ok: boolean; latency_ms: number; model: string };

const PROVIDERS = [
  { value: "huakati", label: "Huakati" },
  { value: "openai-compatible", label: "OpenAI-compatible" },
];

/** Stored skills are opaque JSON; keep the fields this page edits. */
function normalizeSkills(value: unknown): Skill[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((raw) => {
    if (typeof raw !== "object" || raw === null) return [];
    const s = raw as Record<string, unknown>;
    if (typeof s.id !== "string") return [];
    return [
      {
        id: s.id,
        enabled: s.enabled !== false,
        allowed_tools: Array.isArray(s.allowed_tools) ? (s.allowed_tools as string[]) : undefined,
      },
    ];
  });
}

export default function AiSettingsPage() {
  const { t } = useI18N();
  const [loading, setLoading] = useState(true);
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [provider, setProvider] = useState("huakati");
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [mcp, setMcp] = useState(true);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const [testError, setTestError] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  useEffect(() => {
    gw<AiSettings>("api/v1/ai/settings")
      .then((res) => {
        setSettings(res);
        setProvider(res.provider);
        setBaseUrl(res.base_url ?? "");
        setModel(res.model);
        setMcp(res.mcp_enabled);
        const stored = normalizeSkills(res.skills);
        setSkills(stored);
        // The tool registry is the only published catalogue of agent skills.
        gw<{ tools: string[] }>("api/v1/ai/tools")
          .then((tools) => {
            const ids = [...new Set([...tools.tools, ...stored.map((s) => s.id)])];
            setSkills(ids.map((id) => stored.find((s) => s.id === id) ?? { id, enabled: true }));
          })
          .catch(() => {});
      })
      .catch((e) => handleApiError(e, setError))
      .finally(() => setLoading(false));
  }, []);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    setTestResult(null);
    setTestError("");
    try {
      const body: Record<string, unknown> = {
        provider,
        base_url: baseUrl.trim() || null,
        model,
        mcp_enabled: mcp,
        skills,
      };
      if (apiKey.trim()) body.api_key = apiKey.trim();
      const res = await gw<AiSettings>("api/v1/ai/settings", {
        method: "PUT",
        body: JSON.stringify(body),
      });
      setSettings(res);
      setApiKey("");
      setNotice(t("ai.saved"));
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setBusy(false);
    }
  }

  async function testConnection() {
    setTesting(true);
    setTestResult(null);
    setTestError("");
    try {
      setTestResult(await gw<TestResult>("api/v1/ai/settings/test", { method: "POST", body: "{}" }));
    } catch (e) {
      handleApiError(e, setTestError);
    } finally {
      setTesting(false);
    }
  }

  const secretsMissing = settings?.secrets_backend === "none";

  if (loading && !settings) {
    return <p className="spinner">{t("common.loading")}</p>;
  }

  return (
    <>
      <div className="page-head" data-testid="ai-settings">
        <div>
          <h1>{t("ai.title")}</h1>
          <p className="page-sub">{t("ai.subtitle")}</p>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}

      {secretsMissing && (
        <div
          className="card card-pad"
          data-testid="ai-openbao-warning"
          style={{ borderColor: "var(--danger)", background: "#fdeaea" }}
        >
          <h2 style={{ color: "var(--danger)" }}>{t("ai.openbaoTitle")}</h2>
          <p style={{ marginTop: 6 }}>{t("ai.openbaoBody")}</p>
        </div>
      )}

      <form className="card card-pad" onSubmit={save} style={{ display: "grid", gap: 14 }}>
        <div className="card-head" style={{ padding: 0, border: 0 }}>
          <h2>{t("ai.providerSection")}</h2>
          <span className={`badge ${settings?.api_key_set ? "success" : "warning"}`} data-testid="ai-key-status">
            {settings?.api_key_set ? t("ai.keySet") : t("ai.keyNotSet")}
          </span>
        </div>

        <div className="form-row" style={{ gridTemplateColumns: "1fr 1fr" }}>
          <div className="field">
            <label htmlFor="ai-provider">{t("ai.provider")}</label>
            <select
              id="ai-provider"
              data-testid="ai-provider"
              value={provider}
              onChange={(e) => setProvider(e.target.value)}
            >
              {PROVIDERS.map((p) => (
                <option key={p.value} value={p.value}>
                  {p.label}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="ai-model">{t("ai.model")}</label>
            <input
              id="ai-model"
              data-testid="ai-model"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder="qwen2.5-72b-instruct"
              required
            />
          </div>
        </div>

        <div className="field">
          <label htmlFor="ai-base-url">{t("ai.baseUrl")}</label>
          <input
            id="ai-base-url"
            data-testid="ai-base-url"
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder="https://api.example.com/v1"
          />
          <p className="hint">{t("ai.baseUrlHint")}</p>
        </div>

        <div className="field">
          <label htmlFor="ai-key">{t("ai.apiKey")}</label>
          <input
            id="ai-key"
            data-testid="ai-key"
            type="password"
            autoComplete="new-password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            disabled={secretsMissing}
            placeholder={settings?.api_key_set ? t("ai.keySet") : t("ai.keyNotSet")}
          />
          <p className="hint">{secretsMissing ? t("ai.keyDisabled") : t("ai.keyHint")}</p>
        </div>

        <label className="check">
          <input type="checkbox" data-testid="ai-mcp" checked={mcp} onChange={(e) => setMcp(e.target.checked)} />
          <span>{t("ai.mcp")}</span>
        </label>
        <p className="hint">{t("ai.mcpHint")}</p>

        <div>
          <h3>{t("ai.skills")}</h3>
          <p className="hint" style={{ marginTop: 4 }}>
            {t("ai.skillsHint")}
          </p>
          {skills.length === 0 ? (
            <p className="hint" data-testid="ai-skills-empty" style={{ marginTop: 8 }}>
              {t("ai.noSkills")}
            </p>
          ) : (
            <div style={{ display: "grid", gap: 8, marginTop: 8 }}>
              {skills.map((s) => (
                <label className="check" key={s.id}>
                  <input
                    type="checkbox"
                    data-testid={`ai-skill-${s.id}`}
                    checked={s.enabled}
                    onChange={(e) =>
                      setSkills((list) =>
                        list.map((x) => (x.id === s.id ? { ...x, enabled: e.target.checked } : x)),
                      )
                    }
                  />
                  <span>{s.id}</span>
                </label>
              ))}
            </div>
          )}
        </div>

        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
          <button className="btn btn-primary" data-testid="ai-save" type="submit" disabled={busy}>
            {busy ? t("common.loading") : t("ai.save")}
          </button>
          <button className="btn" data-testid="ai-test" type="button" disabled={testing} onClick={testConnection}>
            {testing ? t("common.loading") : t("ai.test")}
          </button>
          <Link className="btn" href="/settings/ai/graph" data-testid="ai-graph-link">
            {t("ai.graph")}
          </Link>
        </div>

        {testResult && (
          <p className="badge success" data-testid="ai-test-result">
            {t("ai.testOk", { ms: testResult.latency_ms, model: testResult.model })}
          </p>
        )}
        {testError && (
          <p className="error" data-testid="ai-test-error">
            {testError}
          </p>
        )}
      </form>
    </>
  );
}
