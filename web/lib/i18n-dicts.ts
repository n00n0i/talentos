import { en, th } from "@/lib/i18n-catalog";
import type { TranslationKey } from "@/lib/i18n-catalog";

export type Dict = Record<string, string>;
export type { TranslationKey };
export { th, en };

/** Keys resolved with the fallback chain, so missing EN never throws. */
const missing = new Set<string>();

/**
 * Explicit fallback: EN → TH → the key itself. Missing keys are collected for
 * `missingKeys()`/tests and logged once in development.
 */
export function translate(
  lang: "th" | "en",
  key: TranslationKey | string,
  vars?: Record<string, string | number>,
): string {
  const value = lang === "en" ? (en[key as TranslationKey] ?? th[key as TranslationKey]) : th[key as TranslationKey];
  if (value === undefined) {
    const first = !missing.has(key);
    missing.add(key);
    if (first && process.env.NODE_ENV !== "production") {
      console.warn(`[i18n] missing key: ${key}`);
    }
    return key;
  }
  return value.replace(/\{(\w+)\}/g, (m, name) =>
    vars?.[name] !== undefined ? String(vars[name]) : m,
  );
}

/**
 * Plural-aware lookup: `key.one` / `key.other` chosen by Intl.PluralRules.
 * Thai has a single "other" form; English uses "one" for 1.
 */
export function plural(
  lang: "th" | "en",
  key: string,
  count: number,
  vars?: Record<string, string | number>,
): string {
  const rule = new Intl.PluralRules(lang === "en" ? "en" : "th-TH").select(count);
  const candidate = `${key}.${rule}`;
  const hasCandidate = candidate in th || (lang === "en" && candidate in en);
  return translate(lang, hasCandidate ? candidate : key, { ...vars, n: count });
}

/** Missing keys observed since load (used by tests and diagnostics). */
export function missingKeys(): string[] {
  return [...missing].sort();
}
