"use client";

import { useEffect } from "react";

/**
 * Adds the header text to every table cell as `data-label`, which the mobile
 * CSS uses to render each row as a stacked card.
 */
export default function TableLabels() {
  useEffect(() => {
    const apply = () => {
      document.querySelectorAll<HTMLTableElement>("table.table").forEach((table) => {
        const headers = [...table.querySelectorAll("thead th")].map(
          (th) => th.textContent?.trim() ?? "",
        );
        table.querySelectorAll<HTMLTableRowElement>("tbody tr").forEach((row) => {
          [...row.children].forEach((cell, index) => {
            if (cell instanceof HTMLElement) cell.dataset.label = headers[index] ?? "";
          });
        });
      });
    };
    apply();
    const observer = new MutationObserver(apply);
    observer.observe(document.body, {
      childList: true,
      subtree: true,
      characterData: true,
    });
    return () => observer.disconnect();
  }, []);
  return null;
}
