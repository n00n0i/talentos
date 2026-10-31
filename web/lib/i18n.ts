"use client";

import { useCallback, useState } from "react";
import { en, th, translate } from "@/lib/i18n-dicts";

export type Lang = "th" | "en";

function readCookie(): Lang {
  if (typeof document === "undefined") return "th";
  const m = document.cookie
    .split("; ")
    .find((s) => s.startsWith("talentos_lang="));
  return m?.split("=")[1] === "en" ? "en" : "th";
}

function setLangCookie(lang: Lang) {
  document.cookie = `talentos_lang=${lang}; path=/; max-age=31536000; SameSite=Lax`;
}

export function useI18N() {
  const [lang, setLangState] = useState<Lang>(readCookie);

  const setLang = useCallback((next: Lang) => {
    setLangCookie(next);
    setLangState(next);
  }, []);

  const t = useCallback(
    (key: string, vars?: Record<string, string | number>) => translate(lang, key, vars),
    [lang],
  );

  return { lang, setLang, t };
}

/** Exposed for unit tests: verifies EN presence with a fallback to Thai. */
export { en, th };
