"use client";

import { useEffect, useRef, useState } from "react";
import {
  parseThaiDateTime,
  parseThaiMonth,
  toThaiDateTime,
  toThaiMonth,
} from "@/lib/thai-date";

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
    const iso = parseThaiMonth(raw);
    if (iso && iso !== value) onChange(iso);
  }
  function blur() {
    const iso = parseThaiMonth(text);
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
    const iso = parseThaiDateTime(raw);
    if (iso && iso !== value) onChange(iso);
  }
  function blur() {
    const iso = parseThaiDateTime(text);
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
