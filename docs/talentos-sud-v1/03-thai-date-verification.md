# SUD-03 — Thai Date Verification

**Gate:** `SUD-A3_DATE_VERIFIED` — **PASS**

## Implementation

- Pure functions: `web/lib/thai-date.ts`
  - `toThaiDate` / `parseThaiDate` (DD/MM/YYYY, Buddhist year +543)
  - `toThaiMonth` / `parseThaiMonth` (MM/YYYY)
  - `toThaiDateTime` / `parseThaiDateTime` (DD/MM/YYYY HH:mm)
- Components: `ThaiDateInput`, `ThaiMonthInput`, `ThaiDateTimeInput`
  - display Buddhist Era; accept Thai or ISO input; ISO values are sent to APIs;
  - native picker button (`showPicker` with click fallback);
  - invalid text resets to the last valid value on blur.
- Replaced all 23 `type=date`, 2 `type=month` and 3 `type=datetime-local` fields.
- No date-only value is passed through a timezone conversion: ISO `YYYY-MM-DD`
  remains the business value; display formatting is local only.

## Unit tests (6 new; 47 vitest total)

- ISO ↔ Buddhist round-trip: `2026-11-04` ↔ `04/11/2569`.
- Leap day: `29/02/2567` ↔ `2024-02-29`; `29/02/2569` rejected.
- Invalid days/months: `31/04/2569`, `00/01/2569`, empty, incomplete, non-date.
- Month boundaries and invalid month 13.
- Date-time: Thai ↔ ISO, 24:00 and 09:60 rejected.
- Test file: `web/test/thai-date.test.ts`; output: `evidence/logs/vitest.log`
  (**47 passed**, including the 6 date cases).

## E2E evidence

- Performance test fills `2026-01-01`, blurs, and asserts **`01/01/2569`**.
- Calendar month field, attendance correction date-time, and interview date-time are
  exercised by the full E2E suite (64/64).
- Screens: payroll/mobile `evidence/screens/admin/390_payroll.png` (Thai date + picker).

## Limitations

- Native picker popup itself follows the browser’s locale; the field text is always
  Thai Buddhist. Headless Chromium cannot verify physical-device pickers.
- Two-digit years are not accepted; users must enter a four-digit Buddhist year.
- No relative-date (“today + 7”) shortcut yet.
