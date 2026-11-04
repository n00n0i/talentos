const pad = (n: number) => String(n).padStart(2, "0");

/** ISO date -> Thai Buddhist display (DD/MM/YYYY+543). */
export function toThaiDate(iso: string): string {
  if (!iso) return "";
  const [y, m, d] = iso.split("-");
  return y && m && d ? `${d}/${m}/${Number(y) + 543}` : "";
}

/** Thai Buddhist display (DD/MM/YYYY+543) or ISO -> ISO date. */
export function parseThaiDate(raw: string): string | null {
  const value = raw.trim();
  if (/^\d{4}-\d{2}-\d{2}$/.test(value)) return value;
  const match = /^(\d{1,2})[/.-](\d{1,2})[/.-](\d{4})$/.exec(value);
  if (!match) return null;
  const day = Number(match[1]);
  const month = Number(match[2]);
  let year = Number(match[3]);
  if (year > 2400) year -= 543;
  const date = new Date(Date.UTC(year, month - 1, day));
  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day
  ) {
    return null;
  }
  return `${year}-${pad(month)}-${pad(day)}`;
}

/** ISO YYYY-MM -> Thai MM/YYYY+543. */
export function toThaiMonth(iso: string): string {
  const [y, m] = iso.split("-");
  return y && m ? `${m}/${Number(y) + 543}` : "";
}

export function parseThaiMonth(raw: string): string | null {
  const value = raw.trim();
  if (/^\d{4}-\d{2}$/.test(value)) return value;
  const match = /^(\d{1,2})\/(\d{4})$/.exec(value);
  if (!match) return null;
  let year = Number(match[2]);
  if (year > 2400) year -= 543;
  const month = Number(match[1]);
  if (month < 1 || month > 12) return null;
  return `${year}-${pad(month)}`;
}

/** ISO date-time (YYYY-MM-DDTHH:mm) -> Thai display. */
export function toThaiDateTime(iso: string): string {
  if (!iso) return "";
  const [date, time = "00:00"] = iso.split("T");
  const [y, m, d] = date.split("-");
  return y && m && d ? `${d}/${m}/${Number(y) + 543} ${time.slice(0, 5)}` : "";
}

export function parseThaiDateTime(raw: string): string | null {
  const value = raw.trim();
  if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(value)) return value.slice(0, 16);
  const match = /^(\d{1,2})[/.-](\d{1,2})[/.-](\d{4})\s+(\d{1,2}):(\d{2})$/.exec(value);
  if (!match) return null;
  const day = Number(match[1]);
  const month = Number(match[2]);
  let year = Number(match[3]);
  if (year > 2400) year -= 543;
  const hour = Number(match[4]);
  const minute = Number(match[5]);
  const date = new Date(Date.UTC(year, month - 1, day, hour, minute));
  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day ||
    hour > 23 ||
    minute > 59
  ) {
    return null;
  }
  return `${year}-${pad(month)}-${pad(day)}T${pad(hour)}:${pad(minute)}`;
}
