import { TH_EN } from "@/lib/th-en";

function english(): boolean {
  return typeof document !== "undefined" && document.documentElement.lang === "en";
}

/** Native prompt with the same Thai→English lookup as the page copy. */
export function uiPrompt(message: string, defaultValue?: string): string | null {
  return window.prompt(english() ? (TH_EN[message] ?? message) : message, defaultValue);
}

/** Native confirm with the same Thai→English lookup as the page copy. */
export function uiConfirm(message: string): boolean {
  return window.confirm(english() ? (TH_EN[message] ?? message) : message);
}
