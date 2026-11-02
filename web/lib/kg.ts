/** Knowledge-graph inspector: ontology classes and response shapes (design §4/§9). */

export const KG_CLASSES = [
  "Organization",
  "LegalEntity",
  "Department",
  "Position",
  "Employee",
  "Skill",
  "Policy",
  "Benefit",
  "PayrollItem",
  "Cycle",
  "Document",
  "Process",
] as const;

export type KgClass = (typeof KG_CLASSES)[number];

export type KgNode = {
  id: string;
  class: KgClass;
  key: string;
  label_th: string;
  label_en: string | null;
  properties: unknown;
};

export type KgEdge = {
  id: string;
  from_id: string;
  to_id: string;
  relation: string;
  properties: unknown;
};

export type KgCounts = {
  nodes: Partial<Record<KgClass, number>>;
  edges: number;
  last_rebuilt_at: string | null;
};

export type KgRebuild = {
  nodes: Partial<Record<KgClass, number>>;
  edges: number;
  capped: boolean;
};

export type KgNeighbors = { nodes: KgNode[]; edges: KgEdge[] };

export type Triple = { id: string; from: KgNode; relation: string; to: KgNode };

export const UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** Every class card is rendered, including classes with zero nodes. */
export function filledCounts(
  nodes: Partial<Record<KgClass, number>> | undefined,
): Record<KgClass, number> {
  const out = {} as Record<KgClass, number>;
  for (const cls of KG_CLASSES) out[cls] = nodes?.[cls] ?? 0;
  return out;
}

export function totalNodes(nodes: Partial<Record<KgClass, number>> | undefined): number {
  return Object.values(filledCounts(nodes)).reduce((a, b) => a + b, 0);
}

export function mergeNodes(current: KgNode[], incoming: KgNode[]): KgNode[] {
  const byId = new Map(current.map((n) => [n.id, n]));
  for (const node of incoming) byId.set(node.id, node);
  return [...byId.values()];
}

export function matchesNode(node: KgNode, query: string, cls?: KgClass | null): boolean {
  if (cls && node.class !== cls) return false;
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [node.id, node.label_th, node.label_en ?? "", node.key, node.class].some((value) =>
    value.toLowerCase().includes(q),
  );
}

/** Edge endpoints missing from the response are dropped rather than rendered blank. */
export function triples(subgraph: KgNeighbors): Triple[] {
  const index = new Map(subgraph.nodes.map((n) => [n.id, n]));
  return subgraph.edges.flatMap((edge) => {
    const from = index.get(edge.from_id);
    const to = index.get(edge.to_id);
    return from && to ? [{ id: edge.id, from, relation: edge.relation, to }] : [];
  });
}
