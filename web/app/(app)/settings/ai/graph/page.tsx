"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { gw, handleApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/i18n-format";
import { useI18N } from "@/lib/i18n";
import {
  filledCounts,
  KG_CLASSES,
  totalNodes,
  triples,
} from "@/lib/kg";
import type {
  KgClass,
  KgCounts,
  KgNeighbors,
  KgNode,
  KgNodeList,
  KgRebuild,
} from "@/lib/kg";

export default function KnowledgeGraphPage() {
  const { t } = useI18N();
  const [loading, setLoading] = useState(true);
  const [counts, setCounts] = useState<KgCounts | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [capped, setCapped] = useState(false);
  const [rebuilding, setRebuilding] = useState(false);

  const [nodes, setNodes] = useState<KgNode[]>([]);
  const [query, setQuery] = useState("");
  const [cls, setCls] = useState<KgClass | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [subgraph, setSubgraph] = useState<KgNeighbors | null>(null);
  const [loadingGraph, setLoadingGraph] = useState(false);
  const [loadingList, setLoadingList] = useState(true);
  const [listVersion, setListVersion] = useState(0);

  useEffect(() => {
    gw<KgCounts>("api/v1/ai/kg")
      .then(setCounts)
      .catch((e) => handleApiError(e, setError))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    let active = true;
    const params = new URLSearchParams();
    const q = query.trim();
    if (q) params.set("q", q);
    if (cls) params.set("class", cls);
    setLoadingList(true);
    gw<KgNodeList>(`api/v1/ai/kg/nodes?${params.toString()}`)
      .then((res) => {
        if (active) setNodes(res.nodes);
      })
      .catch((e) => {
        if (active) handleApiError(e, setError);
      })
      .finally(() => {
        if (active) setLoadingList(false);
      });
    return () => {
      active = false;
    };
  }, [query, cls, listVersion]);

  async function openNode(id: string) {
    setSelected(id);
    setLoadingGraph(true);
    setError("");
    try {
      const res = await gw<KgNeighbors>(`api/v1/ai/kg/neighbors?node_id=${id}&depth=2`);
      if (res.nodes.length === 0) {
        setSubgraph(null);
        setError(t("kg.notFound"));
        return;
      }
      setSubgraph(res);
    } catch (e) {
      setSubgraph(null);
      handleApiError(e, setError);
    } finally {
      setLoadingGraph(false);
    }
  }

  async function rebuild() {
    setRebuilding(true);
    setError("");
    setNotice("");
    setCapped(false);
    try {
      const report = await gw<KgRebuild>("api/v1/ai/kg/rebuild", {
        method: "POST",
        body: "{}",
      });
      setCounts((prev) => ({
        nodes: report.nodes,
        edges: report.edges,
        last_rebuilt_at: prev?.last_rebuilt_at ?? null,
      }));
      setCapped(report.capped);
      setNotice(
        t("kg.rebuilt", { nodes: totalNodes(report.nodes), edges: report.edges }),
      );
      // Rebuild returns counts only; refresh for the server's last_rebuilt_at.
      gw<KgCounts>("api/v1/ai/kg")
        .then(setCounts)
        .catch(() => {});
      setListVersion((v) => v + 1);
    } catch (e) {
      handleApiError(e, setError);
    } finally {
      setRebuilding(false);
    }
  }

  const perClass = useMemo(() => filledCounts(counts?.nodes), [counts]);
  const edges = useMemo(() => (subgraph ? triples(subgraph) : []), [subgraph]);
  const selectedNode =
    nodes.find((node) => node.id === selected) ??
    subgraph?.nodes.find((node) => node.id === selected) ??
    null;

  if (loading && !counts) return <p className="spinner">{t("common.loading")}</p>;

  const empty = totalNodes(counts?.nodes) === 0;

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t("kg.title")}</h1>
          <p className="page-sub">{t("kg.subtitle")}</p>
        </div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button
            className="btn btn-primary"
            data-testid="kg-rebuild"
            type="button"
            disabled={rebuilding}
            onClick={rebuild}
          >
            {rebuilding ? t("kg.rebuilding") : t("kg.rebuild")}
          </button>
          <Link className="btn" href="/settings/ai">
            {t("ai.title")}
          </Link>
        </div>
      </div>

      {error && <p className="error" role="alert">{error}</p>}
      {notice && (
        <p className="badge success" data-testid="kg-rebuilt">
          {notice}
        </p>
      )}
      {capped && <p className="hint">{t("kg.capped")}</p>}

      <p className="hint" style={{ display: "flex", gap: 16, flexWrap: "wrap" }}>
        <span>
          {t("kg.nodes")}: <strong data-testid="kg-total-nodes">{totalNodes(counts?.nodes)}</strong>
        </span>
        <span>
          {t("kg.edges")}: <strong data-testid="kg-total-edges">{counts?.edges ?? 0}</strong>
        </span>
        <span>
          {t("kg.lastRebuilt")}:{" "}
          <strong data-testid="kg-last-rebuilt">
            {counts?.last_rebuilt_at
              ? formatDateTime(counts.last_rebuilt_at)
              : t("kg.never")}
          </strong>
        </span>
      </p>

      <div className="card-head" style={{ padding: "0 0 10px", border: 0 }}>
        <h2>{t("kg.classes")}</h2>
        {cls && (
          <button className="btn btn-sm" type="button" onClick={() => setCls(null)}>
            {t("kg.allClasses")}
          </button>
        )}
      </div>
      <div className="grid-4" data-testid="kg-counts">
        {KG_CLASSES.map((c) => (
          <button
            key={c}
            type="button"
            className="card card-pad"
            data-testid={`kg-card-${c}`}
            onClick={() => setCls(cls === c ? null : c)}
            style={{
              textAlign: "left",
              cursor: "pointer",
              borderColor: cls === c ? "var(--teal)" : undefined,
            }}
          >
            <div className="stat-label">{t(`kg.class.${c}`)}</div>
            <div className="stat-value" data-testid={`kg-count-${c}`}>
              {perClass[c]}
            </div>
            <div className="stat-hint">{c}</div>
          </button>
        ))}
      </div>

      {empty ? (
        <div className="card card-pad" data-testid="kg-empty">
          <h2>{t("kg.emptyTitle")}</h2>
          <p className="hint" style={{ marginTop: 6 }}>
            {t("kg.emptyBody")}
          </p>
          <button
            className="btn btn-primary"
            type="button"
            style={{ marginTop: 14 }}
            disabled={rebuilding}
            onClick={rebuild}
          >
            {rebuilding ? t("kg.rebuilding") : t("kg.rebuild")}
          </button>
        </div>
      ) : (
        <div className="grid-2">
          <div className="card">
            <div className="card-head">
              <h2>{t("kg.nodes")}</h2>
              <span className="badge">{nodes.length}</span>
            </div>
            <form
              onSubmit={(e) => e.preventDefault()}
              style={{ display: "flex", gap: 8, padding: "12px 20px", borderBottom: "1px solid #e7eae3" }}
            >
              <input
                aria-label={t("kg.search")}
                data-testid="kg-search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("kg.searchPlaceholder")}
                style={{ flex: 1, minWidth: 0 }}
              />
            </form>
            {loadingList ? (
              <p className="recent-empty">{t("common.loading")}</p>
            ) : nodes.length === 0 ? (
              <p className="recent-empty" data-testid="kg-no-results">
                {t("kg.noResults")}
              </p>
            ) : (
              <ul className="recent" data-testid="kg-nodes">
                {nodes.map((node) => (
                  <li key={node.id}>
                    <button
                      type="button"
                      className="recent-item"
                      data-testid={`kg-node-${node.id}`}
                      onClick={() => void openNode(node.id)}
                      title={`${node.key} · ${node.id}`}
                      style={{
                        width: "100%",
                        background: "none",
                        border: 0,
                        borderBottom: "1px solid var(--line)",
                        textAlign: "left",
                        cursor: "pointer",
                      }}
                    >
                      <span className="recent-label">{node.label_th}</span>
                      <span className="badge">{t(`kg.class.${node.class}`)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>

          <div className="card">
            <div className="card-head">
              <h2>{t("kg.neighbours")}</h2>
              {selectedNode && <span className="badge accent">{selectedNode.label_th}</span>}
            </div>
            {loadingGraph ? (
              <p className="recent-empty">{t("common.loading")}</p>
            ) : !subgraph ? (
              <p className="recent-empty" data-testid="kg-select-hint">
                {t("kg.selectHint")}
              </p>
            ) : edges.length === 0 ? (
              <p className="recent-empty" data-testid="kg-no-neighbours">
                {t("kg.noNeighbours")}
              </p>
            ) : (
              <table className="table" data-testid="kg-triples">
                <thead>
                  <tr>
                    <th scope="col">{t("kg.subject")}</th>
                    <th scope="col">{t("kg.relation")}</th>
                    <th scope="col">{t("kg.object")}</th>
                  </tr>
                </thead>
                <tbody>
                  {edges.map((edge) => (
                    <tr key={edge.id}>
                      <td>
                        <button
                          type="button"
                          onClick={() => void openNode(edge.from.id)}
                          style={{ background: "none", border: 0, padding: 0, cursor: "pointer", textAlign: "left" }}
                        >
                          {edge.from.label_th}
                        </button>
                      </td>
                      <td>
                        <span className="badge">{edge.relation}</span>
                      </td>
                      <td>
                        <button
                          type="button"
                          onClick={() => void openNode(edge.to.id)}
                          style={{ background: "none", border: 0, padding: 0, cursor: "pointer", textAlign: "left" }}
                        >
                          {edge.to.label_th}
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </div>
      )}
    </>
  );
}
