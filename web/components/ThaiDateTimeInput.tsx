"use client";

import { useEffect, useRef, useState } from "react";

const pad = (n: number) => String(n).padStart(2, "0");

function toThaiMonth(iso: string): string {
  const [y, m] = iso.split("-");
  return y && m ? `${m}/${Number(y) + 543}` : "";
}
function parseMonth(raw: string): string | null {
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

function toThaiDateTime(iso: string): string {
  if (!iso) return "";
  const [date, time = "00:00"] = iso.split("T");
  const [y, m, d] = date.split("-");
  return y && m && d ? `${d}/${m}/${Number(y) + 543} ${time.slice(0, 5)}` : "";
}
function parseDateTime(raw: string): string | null {
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

function picker(ref: React.RefObject<HTMLInputElement | null>) {
  return () => {
    const el = ref.current;
    if (!el) return;
    try {
      if (typeof el.showPicker === "function") el.showPicker();
      else el.click();
    } catch {
      el.click();
    }
  };
}

type Base = {
  id?: string;
  testId?: string;
  required?: boolean;
  ariaLabel?: string;
  disabled?: boolean;
};

/** Month field showing MM/YYYY (Thai Buddhist year). */
export function ThaiMonthInput({
  value,
  onChange,
  id,
  testId,
  required,
  ariaLabel,
  disabled,
}: Base & { value: string; onChange: (iso: string) => void }) {
  const [text, setText] = useState(() => toThaiMonth(value));
  const native = useRef<HTMLInputElement>(null);
  useEffect(() => setText(toThaiMonth(value)), [value]);

  function change(raw: string) {
    setText(raw);
    const iso = parseMonth(raw);
    if (iso && iso !== value) onChange(iso);
  }
  function blur() {
    const iso = parseMonth(text);
    if (iso) {
      setText(toThaiMonth(iso));
      if (iso !== value) onChange(iso);
    } else if (value) setText(toThaiMonth(value));
  }

  return (
    <span className="thai-date">
      <input
        id={id}
        data-testid={testId}
        aria-label={ariaLabel}
        value={text}
        onChange={(e) => change(e.target.value)}
        onBlur={blur}
        placeholder="ดด/ปปปป"
        inputMode="numeric"
        autoComplete="off"
        required={required}
        disabled={disabled}
      />
      <button
        type="button"
        className="thai-date-picker"
        aria-label="เปิดปฏิทิน"
        tabIndex={-1}
        disabled={disabled}
        onClick={picker(native)}
      >
        ▣
      </button>
      <input
        ref={native}
        type="month"
        tabIndex={-1}
        aria-hidden="true"
        className="thai-date-native"
        value={value}
        onChange={(e) => e.target.value && onChange(e.target.value)}
      />
    </span>
  );
}

/** Date/time field showing DD/MM/YYYY HH:mm (Thai Buddhist year). */
export function ThaiDateTimeInput({
  value,
  onChange,
  id,
  testId,
  required,
  ariaLabel,
  disabled,
}: Base & { value: string; onChange: (iso: string) => void }) {
  const [text, setText] = useState(() => toThaiDateTime(value));
  const native = useRef<HTMLInputElement>(null);
  useEffect(() => setText(toThaiDateTime(value)), [value]);

  function change(raw: string) {
    setText(raw);
    const iso = parseDateTime(raw);
    if (iso && iso !== value) onChange(iso);
  }
  function blur() {
    const iso = parseDateTime(text);
    if (iso) {
      setText(toThaiDateTime(iso));
      if (iso !== value) onChange(iso);
    } else if (value) setText(toThaiDateTime(value));
  }

  return (
    <span className="thai-date">
      <input
        id={id}
        data-testid={testId}
        aria-label={ariaLabel}
        value={text}
        onChange={(e) => change(e.target.value)}
        onBlur={blur}
        placeholder="วว/ดด/ปปปป ชช:นน"
        inputMode="numeric"
        autoComplete="off"
        required={required}
        disabled={disabled}
      />
      <button
        type="button"
        className="thai-date-picker"
        aria-label="เปิดปฏิทิน"
        tabIndex={-1}
        disabled={disabled}
        onClick={picker(native)}
      >
        ▣
      </button>
      <input
        ref={native}
        type="datetime-local"
        tabIndex={-1}
        aria-hidden="true"
        className="thai-date-native"
        value={value}
        onChange={(e) => e.target.value && onChange(e.target.value)}
      />
    </span>
  );
}
