import type { Lang } from "@/lib/i18n";

/** Locale-aware date for date-only ISO values (no timezone shift). */
export function formatDate(iso: string, lang: Lang): string {
  if (!iso) return "";
  const [y, m, d] = iso.split("-").map(Number);
  if (!y || !m || !d) return iso;
  return new Date(Date.UTC(y, m - 1, d)).toLocaleDateString(
    lang === "en" ? "en-GB" : "th-TH",
    { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" },
  );
}

export function formatNumber(value: string | number, lang: Lang, digits = 2): string {
  return new Intl.NumberFormat(lang === "en" ? "en-GB" : "th-TH", {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(Number(value));
}

export function formatCurrency(value: string | number, lang: Lang): string {
  return `฿${formatNumber(value, lang)}`;
}
