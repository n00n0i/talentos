import { describe, expect, it } from "vitest";
import { parseThaiDate, toThaiDate } from "@/lib/thai-date";
import {
  parseThaiDateTime,
  parseThaiMonth,
  toThaiDateTime,
  toThaiMonth,
} from "@/lib/thai-date";

describe("Thai Buddhist dates", () => {
  it("stores ISO and displays Buddhist Era", () => {
    expect(toThaiDate("2026-11-04")).toBe("04/11/2569");
    expect(parseThaiDate("04/11/2569")).toBe("2026-11-04");
  });

  it("round-trips leap day in a leap Buddhist year", () => {
    expect(parseThaiDate("29/02/2567")).toBe("2024-02-29");
    expect(toThaiDate("2024-02-29")).toBe("29/02/2567");
  });

  it("rejects a leap day in a non-leap year and out-of-range days", () => {
    expect(parseThaiDate("29/02/2569")).toBeNull();
    expect(parseThaiDate("31/04/2569")).toBeNull();
    expect(parseThaiDate("00/01/2569")).toBeNull();
    expect(parseThaiDate("")).toBeNull();
  });

  it("accepts ISO input and rejects incomplete text", () => {
    expect(parseThaiDate("2024-02-29")).toBe("2024-02-29");
    expect(parseThaiDate("4/11/2569")).toBe("2026-11-04");
    expect(parseThaiDate("04/11")).toBeNull();
    expect(parseThaiDate("abc")).toBeNull();
  });

  it("handles month boundaries", () => {
    expect(toThaiMonth("2026-11")).toBe("11/2569");
    expect(parseThaiMonth("11/2569")).toBe("2026-11");
    expect(parseThaiMonth("2026-12")).toBe("2026-12");
    expect(parseThaiMonth("13/2569")).toBeNull();
    expect(parseThaiMonth("")).toBeNull();
  });

  it("keeps date-time values local and validates time", () => {
    expect(toThaiDateTime("2026-11-04T09:05")).toBe("04/11/2569 09:05");
    expect(parseThaiDateTime("04/11/2569 09:05")).toBe("2026-11-04T09:05");
    expect(parseThaiDateTime("2026-11-04T09:05")).toBe("2026-11-04T09:05");
    expect(parseThaiDateTime("04/11/2569 24:00")).toBeNull();
    expect(parseThaiDateTime("04/11/2569 09:60")).toBeNull();
    expect(parseThaiDateTime("")).toBeNull();
  });
});
