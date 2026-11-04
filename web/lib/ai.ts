export type ModelRole = "chat" | "embedding" | "ocr" | "vision" | "graph_extract";

export const MODEL_ROLES: ModelRole[] = ["chat", "embedding", "ocr", "vision", "graph_extract"];

/** Design §5 capability hints; the UI toggle turns them into advice, not a gate. */
const ROLE_HINTS: Record<Exclude<ModelRole, "chat">, string[]> = {
  embedding: ["embed", "bge", "e5", "nomic"],
  vision: ["vision", "vl", "gpt-4o", "claude-3", "gemini", "llava"],
  ocr: ["ocr", "dots", "got", "nougat"],
  graph_extract: [
    "128k", "200k", "256k", "512k", "1m", "32k",
    "long", "gpt-4", "claude", "gemini", "qwen", "deepseek",
  ],
};

/** Chat is everything that is neither an embedding nor an OCR model. */
export function matchesRole(role: ModelRole, modelId: string): boolean {
  const id = modelId.toLowerCase();
  if (role === "chat") return !matchesRole("embedding", id) && !matchesRole("ocr", id);
  return ROLE_HINTS[role].some((hint) => id.includes(hint));
}

export function modelsForRole(
  role: ModelRole,
  modelIds: string[],
  showAll: boolean,
): string[] {
  if (showAll) return modelIds;
  return modelIds.filter((id) => matchesRole(role, id));
}
