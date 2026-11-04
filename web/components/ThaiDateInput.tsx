"use client";

import { useEffect, useRef, useState } from "react";
import { parseThaiDate, toThaiDate } from "@/lib/thai-date";

type Props = {
  value: string;
  onChange: (iso: string) => void;
  id?: string;
  testId?: string;
  required?: boolean;
  ariaLabel?: string;
  disabled?: boolean;
};

/**
 * Date field that displays and accepts Thai Buddhist dates (DD/MM/YYYY+543).
 * ISO values still work on input, so existing API-shaped values and tests are
 * unaffected. The calendar button opens the native picker.
 */
export default function ThaiDateInput({
  value,
  onChange,
  id,
  testId,
  required,
  ariaLabel,
  disabled,
}: Props) {
  const [text, setText] = useState(() => toThaiDate(value));
  const native = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setText(toThaiDate(value));
  }, [value]);

  function change(raw: string) {
    setText(raw);
    const iso = parseThaiDate(raw);
    if (iso && iso !== value) onChange(iso);
  }

  function blur() {
    const iso = parseThaiDate(text);
    if (iso) {
      setText(toThaiDate(iso));
      if (iso !== value) onChange(iso);
    } else if (value) {
      setText(toThaiDate(value));
    }
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
        placeholder="วว/ดด/ปปปป"
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
        onClick={() => {
          const el = native.current;
          if (!el) return;
          try {
            if (typeof el.showPicker === "function") el.showPicker();
            else el.click();
          } catch {
            el.click();
          }
        }}
      >
        ▣
      </button>
      <input
        ref={native}
        type="date"
        tabIndex={-1}
        aria-hidden="true"
        className="thai-date-native"
        value={value}
        onChange={(e) => {
          if (e.target.value) onChange(e.target.value);
        }}
      />
    </span>
  );
}
