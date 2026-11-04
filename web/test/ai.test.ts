import { describe, expect, it } from "vitest";
import { matchesRole, MODEL_ROLES, modelsForRole } from "@/lib/ai";

describe("model capability filters (design §5)", () => {
  it("keeps embedding models out of chat and matches the embedding hints", () => {
    expect(matchesRole("embedding", "text-embedding-3-small")).toBe(true);
    expect(matchesRole("embedding", "bge-m3")).toBe(true);
    expect(matchesRole("embedding", "nomic-embed-text")).toBe(true);
    expect(matchesRole("embedding", "gpt-4o")).toBe(false);
    expect(matchesRole("chat", "bge-m3")).toBe(false);
    expect(matchesRole("chat", "gpt-4o")).toBe(true);
  });

  it("matches vision and OCR hints", () => {
    for (const id of ["gpt-4o", "claude-3-5-sonnet", "gemini-1.5-pro", "llava:13b", "qwen-vl"]) {
      expect(matchesRole("vision", id), id).toBe(true);
    }
    for (const id of ["dots.ocr", "got-ocr", "nougat-0.1"]) {
      expect(matchesRole("ocr", id), id).toBe(true);
    }
    expect(matchesRole("ocr", "gpt-4o")).toBe(false);
  });

  it("uses a large-context hint for graph extraction", () => {
    expect(matchesRole("graph_extract", "qwen2.5-128k")).toBe(true);
    expect(matchesRole("graph_extract", "deepseek-chat")).toBe(true);
    expect(matchesRole("graph_extract", "bge-m3")).toBe(false);
  });

  it("show-all disables the filter and keeps every model", () => {
    const ids = ["gpt-4o", "bge-m3", "dots.ocr"];
    expect(modelsForRole("embedding", ids, false)).toEqual(["bge-m3"]);
    expect(modelsForRole("embedding", ids, true)).toEqual(ids);
    expect(modelsForRole("chat", ids, false)).toEqual(["gpt-4o"]);
  });

  it("declares all five roles", () => {
    expect(MODEL_ROLES).toEqual(["chat", "embedding", "ocr", "vision", "graph_extract"]);
  });
});
