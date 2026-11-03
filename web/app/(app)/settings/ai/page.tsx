"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { ApiError, gw, handleApiError } from "@/lib/api";
import { MODEL_ROLES, modelsForRole } from "@/lib/ai";
import type { ModelRole } from "@/lib/ai";
import { useI18N } from "@/lib/i18n";

type Skill = { id: string; enabled: boolean; allowed_tools?: string[] };
type ModelInfo = { id: string; owned_by?: string; created?: number };

type Provider = {
  id: string;
  name: string;
  kind: string;
  base_url: string;
  has_key: boolean;
  models_cache: unknown;
  models_fetched_at: string | null;
  enabled: boolean;
  created_at: string;
};

type Preset = { kind: string; label: string; base_url: string | null; key_required: boolean };
type RoleEntry = { provider_id: string; model: string; params?: unknown };
type RoleMap = Record<ModelRole, RoleEntry | null>;
type Agent = { name: string; allowed_tools: string[]; model_role: ModelRole };

type AiSettings = {
  mcp_enabled: boolean;
  skills: unknown;
  agents: unknown;
  secrets_backend: "openbao" | "none";
};

type TestResult = { ok: boolean; latency_ms: number; model: string };

type Tab = "providers" | "roles" | "mcp" | "skills" | "agents";

const TABS: { id: Tab; label: string }[] = [
  { id: "providers", label: "ai.tabs.providers" },
  { id: "roles", label: "ai.tabs.roles" },
  { id: "mcp", label: "ai.tabs.mcp" },
  { id: "skills", label: "ai.tabs.skills" },
  { id: "agents", label: "ai.tabs.agents" },
];

const ROLE_LABEL: Record<ModelRole, string> = {
  chat: "ai.roleChat",
  embedding: "ai.roleEmbedding",
  ocr: "ai.roleOcr",
  vision: "ai.roleVision",
  graph_extract: "ai.roleGraphExtract",
};

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

function normalizeAgents(value: unknown): Agent[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((raw) => {
    if (typeof raw !== "object" || raw === null) return [];
    const a = raw as Record<string, unknown>;
    return [
      {
        name: typeof a.name === "string" ? a.name : "",
        allowed_tools: Array.isArray(a.allowed_tools)
          ? a.allowed_tools.filter((tool): tool is string => typeof tool === "string")
          : [],
        model_role: MODEL_ROLES.includes(a.model_role as ModelRole)
          ? (a.model_role as ModelRole)
          : "chat",
      },
    ];
  });
}

function normalizeModels(value: unknown): ModelInfo[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((raw) => {
    if (typeof raw !== "object" || raw === null) return [];
    const model = raw as Record<string, unknown>;
    return typeof model.id === "string"
      ? [{ id: model.id, owned_by: String(model.owned_by ?? ""), created: Number(model.created ?? 0) }]
      : [];
  });
}

function normalizeRoles(value: Partial<Record<ModelRole, RoleEntry | null>>): RoleMap {
  return {
    chat: value.chat ?? null,
    embedding: value.embedding ?? null,
    ocr: value.ocr ?? null,
    vision: value.vision ?? null,
    graph_extract: value.graph_extract ?? null,
  };
}

function roleKey(providerId: string, model: string) {
  return `${providerId}::${model}`;
}

export default function AiSettingsPage() {
  const { t, lang } = useI18N();
  const [tab, setTab] = useState<Tab>("providers");
  const [loading, setLoading] = useState(true);
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const [providers, setProviders] = useState<Provider[]>([]);
  const [presets, setPresets] = useState<Preset[]>([]);
  const [providerError, setProviderError] = useState("");
  const [providerBusy, setProviderBusy] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [kind, setKind] = useState("openai");
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [fetchingId, setFetchingId] = useState<string | null>(null);
  const [modelErrors, setModelErrors] = useState<Record<string, string>>({});

  const [roles, setRoles] = useState<RoleMap>(normalizeRoles({}));
  const [roleDrafts, setRoleDrafts] = useState<RoleMap>(normalizeRoles({}));
  const [showAll, setShowAll] = useState(false);
  const [savingRole, setSavingRole] = useState<ModelRole | null>(null);
  const [roleErrors, setRoleErrors] = useState<Partial<Record<ModelRole, string>>>({});

  const [mcp, setMcp] = useState(true);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [tools, setTools] = useState<string[]>([]);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [savingTab, setSavingTab] = useState<"mcp" | "skills" | "agents" | null>(null);

  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const [testError, setTestError] = useState("");

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        const [providerRes, presetRes, settingsRes, toolsRes, rolesRes] = await Promise.all([
          gw<{ providers: Provider[] }>("api/v1/ai/providers"),
          gw<{ presets: Preset[] }>("api/v1/ai/provider-presets"),
          gw<AiSettings>("api/v1/ai/settings"),
          gw<{ tools: string[] }>("api/v1/ai/tools"),
          gw<Partial<Record<ModelRole, RoleEntry | null>>>("api/v1/ai/model-roles"),
        ]);
        if (!active) return;
        setProviders(providerRes.providers);
        setPresets(presetRes.presets);
        setKind(presetRes.presets[0]?.kind ?? "openai");
        setBaseUrl(presetRes.presets[0]?.base_url ?? "");
        setSettings(settingsRes);
        setMcp(settingsRes.mcp_enabled);
        setAgents(normalizeAgents(settingsRes.agents));
        setTools(toolsRes.tools);

        const stored = normalizeSkills(settingsRes.skills);
        const ids = [...new Set([...toolsRes.tools, ...stored.map((s) => s.id)])];
        setSkills(ids.map((id) => stored.find((s) => s.id === id) ?? { id, enabled: true }));

        const nextRoles = normalizeRoles(rolesRes);
        // Design §5: graph extraction defaults to the chat model.
        const drafts = { ...nextRoles, graph_extract: nextRoles.graph_extract ?? nextRoles.chat };
        setRoles(nextRoles);
        setRoleDrafts(drafts);
      } catch (e) {
        if (active) handleApiError(e, setError);
      } finally {
        if (active) setLoading(false);
      }
    }
    load();
    return () => {
      active = false;
    };
  }, []);

  function messageOf(e: unknown) {
    if (e instanceof ApiError && e.status === 401) {
      window.location.href = "/login";
      return "";
    }
    return e instanceof Error ? e.message : t("ai.error");
  }

  function resetProviderForm() {
    setEditingId(null);
    setKind(presets[0]?.kind ?? "openai");
    setName("");
    setBaseUrl(presets[0]?.base_url ?? "");
    setApiKey("");
    setProviderError("");
  }

  function startEdit(provider: Provider) {
    setEditingId(provider.id);
    setKind(provider.kind);
    setName(provider.name);
    setBaseUrl(provider.base_url);
    setApiKey("");
    setProviderError("");
  }

  function changeKind(value: string) {
    setKind(value);
    const preset = presets.find((p) => p.kind === value);
    setBaseUrl(preset?.base_url ?? "");
  }

  async function saveProvider(e: React.FormEvent) {
    e.preventDefault();
    setProviderBusy(true);
    setProviderError("");
    setNotice("");
    try {
      const body = {
        name: name.trim(),
        kind,
        base_url: baseUrl.trim(),
        api_key: apiKey.trim() || undefined,
      };
      const saved = editingId
        ? await gw<Provider>(`api/v1/ai/providers/${editingId}`, {
            method: "PUT",
            body: JSON.stringify(body),
          })
        : await gw<Provider>("api/v1/ai/providers", {
            method: "POST",
            body: JSON.stringify(body),
          });
      setProviders((list) =>
        (editingId ? list.map((p) => (p.id === saved.id ? saved : p)) : [...list, saved]).sort(
          (a, b) => a.name.localeCompare(b.name),
        ),
      );
      setNotice(t("ai.providerSaved"));
      resetProviderForm();
    } catch (e) {
      setProviderError(messageOf(e));
    } finally {
      setProviderBusy(false);
    }
  }

  async function removeProvider(provider: Provider) {
    if (!window.confirm(t("ai.confirmDeleteProvider"))) return;
    setError("");
    setNotice("");
    try {
      await gw(`api/v1/ai/providers/${provider.id}`, { method: "DELETE" });
      setProviders((list) => list.filter((p) => p.id !== provider.id));
      setRoles((current) => {
        const next = { ...current };
        for (const role of MODEL_ROLES) {
          if (next[role]?.provider_id === provider.id) next[role] = null;
        }
        return next;
      });
      setRoleDrafts((current) => {
        const next = { ...current };
        for (const role of MODEL_ROLES) {
          if (next[role]?.provider_id === provider.id) next[role] = null;
        }
        return next;
      });
      if (editingId === provider.id) resetProviderForm();
      setNotice(t("ai.providerDeleted"));
    } catch (e) {
      setError(messageOf(e));
    }
  }

  async function fetchModels(provider: Provider) {
    setFetchingId(provider.id);
    setModelErrors((current) => ({ ...current, [provider.id]: "" }));
    setError("");
    try {
      const res = await gw<{ models: ModelInfo[] }>(
        `api/v1/ai/providers/${provider.id}/models`,
        { method: "POST", body: "{}" },
      );
      setProviders((list) =>
        list.map((p) =>
          p.id === provider.id
            ? { ...p, models_cache: res.models, models_fetched_at: new Date().toISOString() }
            : p,
        ),
      );
    } catch (e) {
      setModelErrors((current) => ({ ...current, [provider.id]: messageOf(e) }));
    } finally {
      setFetchingId(null);
    }
  }

  async function saveRole(role: ModelRole) {
    const draft = roleDrafts[role];
    if (!draft?.model) return;
    setSavingRole(role);
    setRoleErrors((current) => ({ ...current, [role]: "" }));
    setNotice("");
    try {
      await gw(`api/v1/ai/model-roles/${role}`, {
        method: "PUT",
        body: JSON.stringify({
          provider_id: draft.provider_id,
          model: draft.model,
          params: draft.params ?? {},
        }),
      });
      setRoles((current) => ({ ...current, [role]: draft }));
      setNotice(t("ai.roleSaved", { role: t(ROLE_LABEL[role]) }));
    } catch (e) {
      setRoleErrors((current) => ({ ...current, [role]: messageOf(e) }));
    } finally {
      setSavingRole(null);
    }
  }

  async function saveSettings(
    key: "mcp" | "skills" | "agents",
    patch: Partial<Pick<AiSettings, "mcp_enabled" | "skills" | "agents">>,
  ) {
    if (!settings) return;
    setSavingTab(key);
    setError("");
    setNotice("");
    try {
      const res = await gw<AiSettings>("api/v1/ai/settings", {
        method: "PUT",
        body: JSON.stringify({
          mcp_enabled: settings.mcp_enabled,
          skills: settings.skills,
          agents: settings.agents,
          ...patch,
        }),
      });
      setSettings(res);
      setNotice(
        t(key === "mcp" ? "ai.mcpSaved" : key === "skills" ? "ai.skillsSaved" : "ai.agentsSaved"),
      );
    } catch (e) {
      setError(messageOf(e));
    } finally {
      setSavingTab(null);
    }
  }

  async function testConnection() {
    setTesting(true);
    setTestResult(null);
    setTestError("");
    try {
      setTestResult(await gw<TestResult>("api/v1/ai/settings/test", { method: "POST", body: "{}" }));
    } catch (e) {
      setTestError(messageOf(e));
    } finally {
      setTesting(false);
    }
  }

  const secretsMissing = settings?.secrets_backend === "none";
  const presetLabel = (value: string) =>
    presets.find((p) => p.kind === value)?.label ?? value;
  const fetchedAt = (iso: string) =>
    new Date(iso).toLocaleString(lang === "en" ? "en-GB" : "th-TH");

  const modelOptions = providers.flatMap((provider) =>
    normalizeModels(provider.models_cache).map((model) => ({
      provider_id: provider.id,
      provider_name: provider.name,
      model: model.id,
    })),
  );

  function optionsForRole(role: ModelRole) {
    const ids = modelsForRole(role, modelOptions.map((o) => o.model), showAll);
    const filtered = modelOptions.filter((o) => ids.includes(o.model));
    const current = roleDrafts[role];
    if (
      current &&
      !filtered.some((o) => o.provider_id === current.provider_id && o.model === current.model)
    ) {
      const saved = modelOptions.find(
        (o) => o.provider_id === current.provider_id && o.model === current.model,
      );
      if (saved) return [saved, ...filtered];
    }
    return filtered;
  }

  if (loading && !settings) {
    return <p className="spinner">{t("common.loading")}</p>;
  }

  return (
    <div className="stack" data-testid="ai-settings">
      <div className="page-head">
        <div>
          <h1>{t("ai.title")}</h1>
          <p className="page-sub">{t("ai.subtitle")}</p>
        </div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button className="btn" data-testid="ai-test" type="button" disabled={testing} onClick={testConnection}>
            {testing ? t("common.loading") : t("ai.test")}
          </button>
          <Link className="btn" href="/settings/ai/graph" data-testid="ai-graph-link">
            {t("ai.graph")}
          </Link>
        </div>
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="badge success">{notice}</p>}
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

      <div role="tablist" style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        {TABS.map((item) => (
          <button
            key={item.id}
            type="button"
            role="tab"
            aria-selected={tab === item.id}
            data-testid={`ai-tab-${item.id}`}
            className={`btn btn-sm ${tab === item.id ? "btn-primary" : ""}`}
            onClick={() => setTab(item.id)}
          >
            {t(`ai.tabs.${item.id}`)}
          </button>
        ))}
      </div>

      {tab === "providers" && (
        <section role="tabpanel" className="stack" data-testid="ai-tabpanel-providers">
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

          <div className="card card-pad" style={{ display: "grid", gap: 12 }}>
            <div className="card-head" style={{ padding: 0, border: 0 }}>
              <h2>{t("ai.providerSection")}</h2>
            </div>
            {providers.length === 0 ? (
              <p className="hint" data-testid="ai-providers-empty">
                {t("ai.noProviders")}
              </p>
            ) : (
              providers.map((provider) => (
                <div
                  key={provider.id}
                  className="card card-pad"
                  data-testid="ai-provider-row"
                  style={{ display: "grid", gap: 10 }}
                >
                  <div
                    style={{
                      display: "flex",
                      justifyContent: "space-between",
                      gap: 12,
                      flexWrap: "wrap",
                    }}
                  >
                    <div style={{ minWidth: 0 }}>
                      <strong>{provider.name}</strong>
                      <span className="chip" style={{ marginLeft: 8 }}>
                        {presetLabel(provider.kind)}
                      </span>
                      <p className="hint" style={{ marginTop: 4, overflowWrap: "anywhere" }}>
                        {provider.base_url}
                      </p>
                    </div>
                    <div style={{ display: "flex", gap: 6, flexWrap: "wrap", alignItems: "start" }}>
                      <span className={`badge ${provider.has_key ? "success" : "warning"}`}>
                        {provider.has_key ? t("ai.keySet") : t("ai.keyNotSet")}
                      </span>
                      <button
                        className="btn btn-sm"
                        type="button"
                        data-testid="ai-provider-edit"
                        onClick={() => startEdit(provider)}
                      >
                        {t("ai.edit")}
                      </button>
                      <button
                        className="btn btn-sm btn-danger"
                        type="button"
                        data-testid="ai-provider-delete"
                        onClick={() => removeProvider(provider)}
                      >
                        {t("ai.delete")}
                      </button>
                    </div>
                  </div>

                  <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                    <button
                      className="btn btn-sm"
                      type="button"
                      data-testid="ai-fetch-models"
                      disabled={fetchingId === provider.id}
                      onClick={() => fetchModels(provider)}
                    >
                      {fetchingId === provider.id ? t("ai.fetchingModels") : t("ai.fetchModels")}
                    </button>
                    {provider.models_fetched_at ? (
                      <>
                        <span className="chip">
                          {t("ai.modelsCount", {
                            n: normalizeModels(provider.models_cache).length,
                          })}
                        </span>
                        <span className="hint">
                          {t("ai.modelsFetchedAt", { at: fetchedAt(provider.models_fetched_at) })}
                        </span>
                      </>
                    ) : (
                      <span className="hint">{t("ai.neverFetched")}</span>
                    )}
                  </div>
                  {modelErrors[provider.id] && (
                    <p className="error" data-testid="ai-provider-error">
                      {modelErrors[provider.id]}
                    </p>
                  )}
                </div>
              ))
            )}
          </div>

          <form
            className="card card-pad"
            onSubmit={saveProvider}
            style={{ display: "grid", gap: 14 }}
          >
            <div className="card-head" style={{ padding: 0, border: 0 }}>
              <h2>{editingId ? t("ai.editProvider") : t("ai.addProvider")}</h2>
            </div>

            <div className="form-row">
              <div className="field">
                <label htmlFor="ai-provider-preset">{t("ai.preset")}</label>
                <select
                  id="ai-provider-preset"
                  data-testid="ai-provider-kind"
                  value={kind}
                  onChange={(e) => changeKind(e.target.value)}
                >
                  {presets.map((preset) => (
                    <option key={preset.kind} value={preset.kind}>
                      {preset.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label htmlFor="ai-provider-name">{t("ai.providerName")}</label>
                <input
                  id="ai-provider-name"
                  data-testid="ai-provider-name"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="OpenAI"
                  required
                />
              </div>
            </div>

            <div className="field">
              <label htmlFor="ai-provider-base-url">{t("ai.baseUrl")}</label>
              <input
                id="ai-provider-base-url"
                data-testid="ai-provider-base-url"
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="https://api.example.com/v1"
                required
              />
              <p className="hint">{t("ai.baseUrlHint")}</p>
            </div>

            <div className="field">
              <label htmlFor="ai-provider-key">
                {t("ai.apiKey")} <span className="hint">{t("ai.optional")}</span>
              </label>
              <input
                id="ai-provider-key"
                data-testid="ai-provider-key"
                type="password"
                autoComplete="new-password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                disabled={secretsMissing}
                placeholder={t("ai.keyNotSet")}
              />
              <p className="hint">{t("ai.apiKeyOptionalHint")}</p>
            </div>

            {providerError && (
              <p className="error" data-testid="ai-provider-form-error">
                {providerError}
              </p>
            )}

            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              <button
                className="btn btn-primary"
                data-testid="ai-provider-save"
                type="submit"
                disabled={providerBusy}
              >
                {providerBusy ? t("common.loading") : t("ai.saveProvider")}
              </button>
              {editingId && (
                <button className="btn" type="button" onClick={resetProviderForm}>
                  {t("common.cancel")}
                </button>
              )}
            </div>
          </form>
        </section>
      )}

      {tab === "roles" && (
        <section role="tabpanel" className="card card-pad" data-testid="ai-tabpanel-roles">
          <div className="card-head" style={{ padding: 0, border: 0 }}>
            <h2>{t("ai.tabs.roles")}</h2>
            <label className="check">
              <input
                type="checkbox"
                data-testid="ai-show-all-models"
                checked={showAll}
                onChange={(e) => setShowAll(e.target.checked)}
              />
              <span>{t("ai.showAllModels")}</span>
            </label>
          </div>
          <p className="hint" style={{ marginTop: 6 }}>
            {t("ai.rolesHint")}
          </p>
          {modelOptions.length === 0 && (
            <p className="hint" style={{ marginTop: 8 }} data-testid="ai-roles-no-models">
              {t("ai.noModelsHint")}
            </p>
          )}

          <div style={{ display: "grid", gap: 16, marginTop: 12 }}>
            {MODEL_ROLES.map((role) => {
              const draft = roleDrafts[role];
              return (
                <div key={role} style={{ display: "grid", gap: 6 }} data-testid={`ai-role-${role}`}>
                  <div
                    style={{
                      display: "flex",
                      gap: 8,
                      alignItems: "center",
                      justifyContent: "space-between",
                    }}
                  >
                    <label htmlFor={`ai-role-select-${role}`} style={{ fontWeight: 600 }}>
                      {t(ROLE_LABEL[role])}
                    </label>
                    {!draft?.model && (
                      <span className="badge warning" data-testid={`ai-role-warning-${role}`}>
                        {t("ai.roleMissing")}
                      </span>
                    )}
                  </div>
                  <div className="form-row" style={{ gridTemplateColumns: "1fr auto" }}>
                    <select
                      id={`ai-role-select-${role}`}
                      data-testid={`ai-role-select-${role}`}
                      value={draft ? roleKey(draft.provider_id, draft.model) : ""}
                      onChange={(e) => {
                        const option = optionsForRole(role).find(
                          (o) => roleKey(o.provider_id, o.model) === e.target.value,
                        );
                        setRoleDrafts((current) => ({
                          ...current,
                          [role]: option
                            ? {
                                provider_id: option.provider_id,
                                model: option.model,
                                params: current[role]?.params ?? {},
                              }
                            : null,
                        }));
                      }}
                    >
                      <option value="">{t("ai.chooseModel")}</option>
                      {optionsForRole(role).map((option) => (
                        <option
                          key={roleKey(option.provider_id, option.model)}
                          value={roleKey(option.provider_id, option.model)}
                        >
                          {`${option.provider_name} · ${option.model}`}
                        </option>
                      ))}
                    </select>
                    <button
                      className="btn btn-sm"
                      type="button"
                      data-testid={`ai-role-save-${role}`}
                      disabled={!draft?.model || savingRole === role}
                      onClick={() => saveRole(role)}
                    >
                      {savingRole === role ? t("common.loading") : t("ai.save")}
                    </button>
                  </div>
                  {roleErrors[role] && <p className="error">{roleErrors[role]}</p>}
                  {roles[role]?.model && roles[role]?.model !== draft?.model && (
                    <p className="hint">{t("ai.roleUnsaved")}</p>
                  )}
                </div>
              );
            })}
          </div>
        </section>
      )}

      {tab === "mcp" && (
        <section role="tabpanel" className="card card-pad" data-testid="ai-tabpanel-mcp">
          <div style={{ display: "grid", gap: 12 }}>
            <h2>{t("ai.mcp")}</h2>
            <label className="check">
              <input
                type="checkbox"
                data-testid="ai-mcp"
                checked={mcp}
                onChange={(e) => setMcp(e.target.checked)}
              />
              <span>{t("ai.mcp")}</span>
            </label>
            <p className="hint">{t("ai.mcpHint")}</p>
            <div>
              <button
                className="btn btn-primary"
                data-testid="ai-save-mcp"
                type="button"
                disabled={savingTab === "mcp"}
                onClick={() => saveSettings("mcp", { mcp_enabled: mcp })}
              >
                {savingTab === "mcp" ? t("common.loading") : t("ai.save")}
              </button>
            </div>
          </div>
        </section>
      )}

      {tab === "skills" && (
        <section role="tabpanel" className="card card-pad" data-testid="ai-tabpanel-skills">
          <div style={{ display: "grid", gap: 12 }}>
            <h2>{t("ai.skills")}</h2>
            <p className="hint">{t("ai.skillsHint")}</p>
            {skills.length === 0 ? (
              <p className="hint" data-testid="ai-skills-empty">
                {t("ai.noSkills")}
              </p>
            ) : (
              <div style={{ display: "grid", gap: 8 }}>
                {skills.map((skill) => (
                  <label className="check" key={skill.id}>
                    <input
                      type="checkbox"
                      data-testid={`ai-skill-${skill.id}`}
                      checked={skill.enabled}
                      onChange={(e) =>
                        setSkills((list) =>
                          list.map((x) =>
                            x.id === skill.id ? { ...x, enabled: e.target.checked } : x,
                          ),
                        )
                      }
                    />
                    <span>{skill.id}</span>
                  </label>
                ))}
              </div>
            )}
            <div>
              <button
                className="btn btn-primary"
                data-testid="ai-save-skills"
                type="button"
                disabled={savingTab === "skills"}
                onClick={() => saveSettings("skills", { skills })}
              >
                {savingTab === "skills" ? t("common.loading") : t("ai.save")}
              </button>
            </div>
          </div>
        </section>
      )}

      {tab === "agents" && (
        <section role="tabpanel" className="card card-pad" data-testid="ai-tabpanel-agents">
          <div style={{ display: "grid", gap: 12 }}>
            <h2>{t("ai.tabs.agents")}</h2>
            <p className="hint">{t("ai.agentsHint")}</p>
            {agents.length === 0 && (
              <p className="hint" data-testid="ai-agents-empty">
                {t("ai.noAgents")}
              </p>
            )}
            {agents.map((agent, index) => (
              <div
                key={index}
                className="card card-pad"
                data-testid="ai-agent-row"
                style={{ display: "grid", gap: 10 }}
              >
                <div className="form-row">
                  <div className="field">
                    <label htmlFor={`ai-agent-name-${index}`}>{t("ai.agentName")}</label>
                    <input
                      id={`ai-agent-name-${index}`}
                      data-testid="ai-agent-name"
                      value={agent.name}
                      onChange={(e) =>
                        setAgents((list) =>
                          list.map((a, i) => (i === index ? { ...a, name: e.target.value } : a)),
                        )
                      }
                      placeholder="PolicyQuery"
                    />
                  </div>
                  <div className="field">
                    <label htmlFor={`ai-agent-role-${index}`}>{t("ai.agentModelRole")}</label>
                    <select
                      id={`ai-agent-role-${index}`}
                      data-testid="ai-agent-role"
                      value={agent.model_role}
                      onChange={(e) =>
                        setAgents((list) =>
                          list.map((a, i) =>
                            i === index ? { ...a, model_role: e.target.value as ModelRole } : a,
                          ),
                        )
                      }
                    >
                      {MODEL_ROLES.map((role) => (
                        <option key={role} value={role}>
                          {t(ROLE_LABEL[role])}
                        </option>
                      ))}
                    </select>
                  </div>
                </div>

                <div className="field">
                  <label htmlFor={`ai-agent-tools-${index}`}>{t("ai.agentTools")}</label>
                  <select
                    id={`ai-agent-tools-${index}`}
                    data-testid="ai-agent-tools"
                    multiple
                    value={agent.allowed_tools}
                    onChange={(e) =>
                      setAgents((list) =>
                        list.map((a, i) =>
                          i === index
                            ? {
                                ...a,
                                allowed_tools: Array.from(
                                  e.target.selectedOptions,
                                  (option) => option.value,
                                ),
                              }
                            : a,
                        ),
                      )
                    }
                    style={{ height: "auto", minHeight: 96, padding: 8 }}
                  >
                    {[...new Set([...tools, ...agent.allowed_tools])].map((tool) => (
                      <option key={tool} value={tool}>
                        {tool}
                      </option>
                    ))}
                  </select>
                </div>

                <div>
                  <button
                    className="btn btn-sm btn-danger"
                    type="button"
                    data-testid="ai-agent-remove"
                    onClick={() => setAgents((list) => list.filter((_, i) => i !== index))}
                  >
                    {t("ai.removeAgent")}
                  </button>
                </div>
              </div>
            ))}
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              <button
                className="btn"
                type="button"
                data-testid="ai-agent-add"
                onClick={() =>
                  setAgents((list) => [
                    ...list,
                    { name: "", allowed_tools: [], model_role: "chat" },
                  ])
                }
              >
                {t("ai.addAgent")}
              </button>
              <button
                className="btn btn-primary"
                data-testid="ai-save-agents"
                type="button"
                disabled={savingTab === "agents"}
                onClick={() => saveSettings("agents", { agents })}
              >
                {savingTab === "agents" ? t("common.loading") : t("ai.save")}
              </button>
            </div>
          </div>
        </section>
      )}
    </div>
  );
}
