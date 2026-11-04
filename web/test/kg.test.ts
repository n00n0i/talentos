import { describe, expect, it } from "vitest";
import { filledCounts, KG_CLASSES, totalNodes, triples } from "@/lib/kg";
import type { KgNode } from "@/lib/kg";

const node = (id: string, cls: KgNode["class"], label: string): KgNode => ({
  id,
  class: cls,
  key: `${cls}-${id}`,
  label_th: label,
  label_en: null,
  properties: {},
});

const employee = node("11111111-1111-1111-1111-111111111111", "Employee", "สมชาย ใจดี");
const department = node("22222222-2222-2222-2222-222222222222", "Department", "ฝ่ายขาย");

describe("filledCounts", () => {
  it("returns all 12 classes, defaulting missing ones to 0", () => {
    const counts = filledCounts({ Employee: 2 });
    expect(Object.keys(counts)).toHaveLength(KG_CLASSES.length);
    expect(counts.Employee).toBe(2);
    expect(counts.PayrollItem).toBe(0);
    expect(totalNodes(counts)).toBe(2);
  });
});

describe("triples", () => {
  it("renders subject → relation → object and drops dangling edges", () => {
    const result = triples({
      nodes: [employee, department],
      edges: [
        { id: "e1", from_id: employee.id, to_id: department.id, relation: "member_of", properties: {} },
        { id: "e2", from_id: employee.id, to_id: "missing", relation: "member_of", properties: {} },
      ],
    });
    expect(result).toHaveLength(1);
    expect(result[0].from.label_th).toBe("สมชาย ใจดี");
    expect(result[0].relation).toBe("member_of");
    expect(result[0].to.label_th).toBe("ฝ่ายขาย");
  });
});
