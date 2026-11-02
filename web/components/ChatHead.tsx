"use client";

import Link from "next/link";
import { useEffect, useRef, useState } from "react";
import Icon from "@/components/Icon";
import { ApiError, gw } from "@/lib/api";
import { useI18N } from "@/lib/i18n";

type Citation = { class: string; label: string; relation: string };
type ChatReply = { reply: string; citations: Citation[]; usage: unknown };
type Message = { role: "user" | "assistant"; text: string; citations?: Citation[] };

export default function ChatHead() {
  const { t } = useI18N();
  const [open, setOpen] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [unavailable, setUnavailable] = useState(false);
  const [forbidden, setForbidden] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);

  // Same pattern as the sidebar drawer in Shell.tsx.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  useEffect(() => {
    listRef.current?.scrollTo({ top: listRef.current.scrollHeight });
  }, [messages, busy]);

  async function send(e: React.FormEvent) {
    e.preventDefault();
    const message = draft.trim();
    if (!message || busy) return;
    setDraft("");
    setUnavailable(false);
    setForbidden(false);
    setMessages((list) => [...list, { role: "user", text: message }]);
    setBusy(true);
    try {
      const res = await gw<ChatReply>("api/v1/ai/chat", {
        method: "POST",
        body: JSON.stringify({ message }),
      });
      setMessages((list) => [
        ...list,
        { role: "assistant", text: res.reply, citations: res.citations },
      ]);
    } catch (err) {
      if (err instanceof ApiError && err.status === 503) {
        setUnavailable(true);
      } else if (err instanceof ApiError && err.status === 403) {
        setForbidden(true);
      } else {
        setMessages((list) => [
          ...list,
          { role: "assistant", text: err instanceof Error ? err.message : t("chat.error") },
        ]);
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      {open && (
        <section
          className="chat-panel"
          data-testid="chat-panel"
          role="dialog"
          aria-label={t("chat.title")}
        >
          <div className="chat-head">
            <strong>{t("chat.title")}</strong>
            <button
              className="icon-btn"
              type="button"
              aria-label={t("chat.close")}
              onClick={() => setOpen(false)}
            >
              ×
            </button>
          </div>
          <div className="chat-list" ref={listRef} role="log" aria-live="polite">
            {messages.length === 0 && !unavailable && !forbidden && (
              <p className="hint">{t("chat.hint")}</p>
            )}
            {forbidden && (
              <p className="error" data-testid="chat-forbidden">
                {t("chat.forbidden")}
              </p>
            )}
            {unavailable && (
              <p className="error" data-testid="chat-unavailable">
                {t("chat.unavailable")}{" "}
                <Link
                  href="/settings/ai"
                  data-testid="chat-settings-link"
                  onClick={() => setOpen(false)}
                >
                  {t("chat.settingsLink")}
                </Link>
              </p>
            )}
            {messages.map((m, i) => (
              <div key={i} className={`chat-msg ${m.role}`}>
                {m.role === "assistant" && !m.text.trim() ? t("chat.emptyReply") : m.text}
                {m.citations && m.citations.length > 0 && (
                  <div className="chat-citations">
                    {m.citations.map((c, j) => (
                      <span className="chip" key={j} title={c.class}>
                        {c.relation} · {c.label}
                      </span>
                    ))}
                  </div>
                )}
              </div>
            ))}
            {busy && <div className="chat-msg assistant chat-thinking">{t("chat.thinking")}</div>}
          </div>
          <form className="chat-form" onSubmit={send}>
            <input
              data-testid="chat-input"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder={t("chat.placeholder")}
              aria-label={t("chat.placeholder")}
              disabled={forbidden}
            />
            <button
              className="btn btn-primary"
              data-testid="chat-send"
              type="submit"
              disabled={busy || forbidden || !draft.trim()}
            >
              {t("chat.send")}
            </button>
          </form>
        </section>
      )}
      <button
        className="chat-launcher"
        data-testid="chat-launcher"
        type="button"
        aria-label={t("chat.open")}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <Icon name="sparkle" size={20} />
      </button>
    </>
  );
}
