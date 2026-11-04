"use client";

import { useCallback, useEffect, useState } from "react";
import { en, plural, th, translate } from "@/lib/i18n-dicts";
import type { TranslationKey } from "@/lib/i18n-catalog";

export type Lang = "th" | "en";

const LANG_EVENT = "talentos-lang";

// Pure helpers (module maps, CSV/validation helpers, server metadata) need a
// translation without a hook. This mirrors the active UI language.
let currentLang: Lang = "th";

export function setCurrentLanguage(lang: Lang) {
  currentLang = lang;
}

export function currentLanguage(): Lang {
  return currentLang;
}

/** Translate with the active language for code that cannot use a hook. */
export function tNow(key: TranslationKey, vars?: Record<string, string | number>): string {
  return translate(currentLang, key, vars);
}

function readCookie(): Lang {
  if (typeof document === "undefined") return "th";
  const m = document.cookie
    .split("; ")
    .find((s) => s.startsWith("talentos_lang="));
  return m?.split("=")[1] === "en" ? "en" : "th";
}

function setLangCookie(lang: Lang) {
  document.cookie = `talentos_lang=${lang}; path=/; max-age=31536000; SameSite=Lax`;
  setCurrentLanguage(lang);
  // Long-lived client components (ChatHead lives in the app layout) need to
  // re-read the cookie when another component toggles the language.
  window.dispatchEvent(new Event(LANG_EVENT));
}

export function useI18N() {
  // Start with Thai on the server and first client render to keep hydration
  // deterministic; read the cookie immediately after mount.
  const [lang, setLangState] = useState<Lang>("th");

  useEffect(() => {
    const sync = () => {
      const next = readCookie();
      setCurrentLanguage(next);
      setLangState(next);
    };
    sync();
    window.addEventListener(LANG_EVENT, sync);
    return () => window.removeEventListener(LANG_EVENT, sync);
  }, []);

  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  const setLang = useCallback((next: Lang) => {
    setLangCookie(next);
    setLangState(next);
  }, []);

  const t = useCallback(
    (key: TranslationKey, vars?: Record<string, string | number>) => translate(lang, key, vars),
    [lang],
  );
  const tDynamic = useCallback(
    (key: string, vars?: Record<string, string | number>) => translate(lang, key, vars),
    [lang],
  );
  const tp = useCallback(
    (key: string, count: number, vars?: Record<string, string | number>) => plural(lang, key, count, vars),
    [lang],
  );

  return { lang, setLang, t, tDynamic, tp };
}

/** Exposed for unit tests: verifies EN presence with a fallback to Thai. */
export { en, th };
