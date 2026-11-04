"use client";

import { useEffect } from "react";
import { useI18N } from "@/lib/i18n";
import { TH_EN } from "@/lib/th-en";

const ATTRS = ["placeholder", "title", "aria-label", "alt"] as const;
const SKIP_TAGS = new Set(["SCRIPT", "STYLE", "CODE", "PRE"]);

const textOriginal = new WeakMap<Text, string>();
const attrOriginal = new WeakMap<Element, Record<string, string>>();

function translateText(text: string): string {
  const match = /^(\s*)([\s\S]*?)(\s*)$/.exec(text);
  if (!match) return text;
  const key = match[2].replace(/\s+/g, " ").trim();
  if (!key) return text;
  const hit = TH_EN[key];
  return hit ? `${match[1]}${hit}${match[3]}` : text;
}

function skip(element: Element | null): boolean {
  if (!element) return false;
  if (SKIP_TAGS.has(element.tagName)) return true;
  return element.closest("[data-no-i18n]") !== null;
}

function applyAttributes(root: HTMLElement) {
  root.querySelectorAll<HTMLElement>("[placeholder],[title],[aria-label],[alt]").forEach((el) => {
    if (skip(el)) return;
    let originals = attrOriginal.get(el);
    if (!originals) {
      originals = {};
      attrOriginal.set(el, originals);
    }
    for (const attr of ATTRS) {
      const current = el.getAttribute(attr);
      if (current === null) continue;
      if (!(attr in originals)) originals[attr] = current;
      const next = translateText(originals[attr]);
      if (next !== current) el.setAttribute(attr, next);
    }
  });
}

function apply(root: HTMLElement) {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const text = node as Text;
    if (skip(text.parentElement)) continue;
    const current = text.nodeValue ?? "";
    if (!textOriginal.has(text)) textOriginal.set(text, current);
    const next = translateText(textOriginal.get(text) ?? current);
    if (next !== current) text.nodeValue = next;
  }
  applyAttributes(root);
}

function restore(root: HTMLElement) {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const text = node as Text;
    const original = textOriginal.get(text);
    if (original !== undefined && text.nodeValue !== original) text.nodeValue = original;
  }
  root.querySelectorAll<HTMLElement>("[placeholder],[title],[aria-label],[alt]").forEach((el) => {
    const originals = attrOriginal.get(el);
    if (!originals) return;
    for (const attr of ATTRS) {
      const original = originals[attr];
      if (original !== undefined && el.getAttribute(attr) !== original) {
        el.setAttribute(attr, original);
      }
    }
  });
}

/**
 * Translate the rendered page in EN mode without touching every page. Thai mode
 * restores the original strings. Runs after mount and follows DOM updates.
 */
export default function Translate({ children }: { children: React.ReactNode }) {
  const { lang } = useI18N();

  useEffect(() => {
    const root = document.querySelector<HTMLElement>("main.content");
    if (!root) return;
    if (lang === "th") {
      restore(root);
      return;
    }
    apply(root);
    let frame = 0;
    const observer = new MutationObserver(() => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        apply(root);
      });
    });
    observer.observe(root, { subtree: true, childList: true, characterData: true });
    return () => {
      observer.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }, [lang]);

  return <>{children}</>;
}
