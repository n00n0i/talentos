"use client";

import { useEffect, useRef, useState } from "react";

const pad = (n: number) => String(n).padStart(2, "0");

function toThai(iso: string): string {
  if (!iso) return "";
  const [y, m, d] = iso.split("-");
  if (!y || !m || !d) return "";
  return `${d}/${m}/${Number(y) + 543}`;
}

function parse(raw: string): string | null {
  const value = raw.trim();
  if (/^\d{4}-\d{2}-\d{2}$/.test(value)) return value;
  const match = /^(\d{1,2})[/.-](\d{1,2})[/.-](\d{4})$/.exec(value);
  if (!match) return null;
  const day = Number(match[1]);
  const month = Number(match[2]);
  let year = Number(match[3]);
  if (year > 2400) year -= 543;
  const date = new Date(Date.UTC(year, month - 1, day));
  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day
  ) {
    return null;
  }
  return `${year}-${pad(month)}-${pad(day)}`;
}

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
  const [text, setText] = useState(() => toThai(value));
  const native = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setText(toThai(value));
  }, [value]);

  function change(raw: string) {
    setText(raw);
    const iso = parse(raw);
    if (iso && iso !== value) onChange(iso);
  }

  function blur() {
    const iso = parse(text);
    if (iso) {
      setText(toThai(iso));
      if (iso !== value) onChange(iso);
    } else if (value) {
      setText(toThai(value));
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
