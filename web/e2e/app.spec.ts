import { createHmac } from "node:crypto";
import { readFileSync } from "node:fs";
import { expect, Page, test } from "@playwright/test";

test.describe.configure({ mode: "serial" });

let invitedEmail = "";
const managerEmail = "manager-e2e@example.com";
const SETUP_TOKEN = process.env.E2E_SETUP_TOKEN ?? "e2e-setup-token";

async function login(page: Page) {
  await page.goto("/login");
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill("e2e@example.com");
  await page.getByLabel("รหัสผ่าน").fill("E2ePassword123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
}

async function apiPost(page: Page, path: string, data: object) {
  return page.evaluate(
    async ({ p, d }) => {
      const res = await fetch(p, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(d),
      });
      const json = await res.json().catch(() => undefined);
      return { ok: res.ok, status: res.status, json };
    },
    { p: path, d: data }
  );
}

async function apiGet(page: Page, path: string) {
  return page.evaluate(async ({ p }) => {
    const res = await fetch(p);
    const json = await res.json().catch(() => undefined);
    return { ok: res.ok, status: res.status, json };
  }, { p: path });
}

test("first-run setup creates the tenant and admin", async ({ page }) => {
  await page.goto("/setup");
  await page.getByTestId("setup-org").fill("E2E Co");
  await page.getByTestId("setup-slug").fill("e2e");
  await page.getByTestId("setup-entity").fill("E2E Main");
  await page.getByTestId("setup-email").fill("e2e@example.com");
  await page.getByTestId("setup-password").fill("E2ePassword123!");
  await page.getByTestId("setup-confirm").fill("E2ePassword123!");
  await page.getByTestId("setup-token").fill(SETUP_TOKEN);
  await page.getByTestId("setup-submit").click();
  await expect(page.getByRole("heading", { name: "เข้าสู่ระบบ" })).toBeVisible();
  await login(page);
});

test("settings persist across reload", async ({ page }) => {
  await login(page);
  await page.goto("/settings");
  await expect(page.getByRole("heading", { name: "สถานะระบบ" })).toBeVisible();
  await expect(page.getByText("การแยกข้อมูล (RLS)")).toBeVisible();
  await expect(page.getByText("พร้อม").first()).toBeVisible();

  await page.getByTestId("divisor").fill("26");
  await page.getByTestId("save-policy").click();
  await expect(page.getByText("บันทึกสูตรเงินเดือนแล้ว")).toBeVisible();

  await page.reload();
  await expect(page.getByTestId("divisor")).toHaveValue("26.00");

  await page.getByTestId("divisor").fill("30");
  await page.getByTestId("save-policy").click();
  await expect(page.getByText("บันทึกสูตรเงินเดือนแล้ว")).toBeVisible();

  await page.getByTestId("gl-salary").fill("6100");
  await page.getByTestId("save-policy").click();
  await expect(page.getByText("บันทึกสูตรเงินเดือนแล้ว")).toBeVisible();

  await page.reload();
  await expect(page.getByTestId("gl-salary")).toHaveValue("6100");

  await page.getByTestId("gl-salary").fill("5100");
  await page.getByTestId("save-policy").click();
  await expect(page.getByText("บันทึกสูตรเงินเดือนแล้ว")).toBeVisible();

  await page.goto("/audit");
  await expect(page.getByRole("heading", { name: "ประวัติการใช้งาน" })).toBeVisible();
  await expect(page.getByText("settings.payroll").first()).toBeVisible();
});

test("creates an employee, records OT, runs and approves payroll", async ({ page }) => {
  await login(page);

  await page.goto("/employees");
  await page.getByTestId("add-employee").click();
  await page.getByTestId("employee-code").fill("E2E001");
  await page.getByTestId("employee-first-name").fill("สมชาย");
  await page.getByTestId("employee-last-name").fill("ทดสอบ");
  await page.getByTestId("employee-position").fill("Engineer");
  await page.getByTestId("employee-salary").fill("40000");
  await page.getByTestId("employee-hire-date").fill("2026-01-01");
  await page.getByTestId("save-employee").click();
  await expect(page.getByText("เพิ่มพนักงานแล้ว")).toBeVisible();
  await expect(page.getByRole("cell", { name: "E2E001" })).toBeVisible();

  // Derived path: shift + device import (09:00-22:00 = 4h after shift end), then HR approval.
  const shift = await apiPost(page, "/api/gw/api/v1/shifts", {
    name: "E2E OT", start: "09:00", end: "18:00", break_minutes: 60, grace_minutes: 0,
  });
  expect(shift.ok).toBeTruthy();
  const { id: shiftId } = shift.json;
  const found = await apiGet(page, "/api/gw/api/v1/employees?q=E2E001");
  const employeeId = found.json.employees[0].id;
  const assign = await apiPost(page, `/api/gw/api/v1/shifts/${shiftId}/assign`, {
    employee_id: employeeId, effective_from: "2026-01-01",
  });
  expect(assign.ok).toBeTruthy();
  const imported = await apiPost(page, "/api/gw/api/v1/attendance/import", {
    rows: [{ employee_code: "E2E001", date: "2026-10-05", in: "09:00", out: "22:00" }],
  });
  expect(imported.json.imported).toBe(1);

  await page.goto("/attendance");
  await page.getByTestId("month-picker").fill("2026-10");
  const otRow = page.getByTestId("day-row").filter({ hasText: "2026-10-05" });
  await expect(otRow).toContainText("4:00");
  await otRow.getByTestId("approve-day").click();
  await expect(page.getByText("อนุมัติวันทำงานแล้ว")).toBeVisible();
  await expect(otRow).toContainText("อนุมัติแล้ว");

  await page.goto("/payroll");
  await page.getByTestId("create-run").click();
  await expect(page.getByText("สร้างรอบเงินเดือนแล้ว")).toBeVisible();
  await expect(page.getByText("รออนุมัติ")).toBeVisible({ timeout: 45_000 });

  // The run carries an anomaly (SSO cap on a 40k salary), so it must be reviewed first.
  await page.getByText("รออนุมัติ").first().click();
  await page.getByTestId("review-anomalies").click();
  await expect(page.getByText("ตรวจสอบความผิดปกติแล้ว")).toBeVisible();

  await page.getByTestId("approve-run").click();
  await expect(page.getByText("อนุมัติรอบเงินเดือนแล้ว")).toBeVisible();
  await expect(page.getByText("อนุมัติแล้ว").first()).toBeVisible();

  // Bank payout: download link is visible and yields a CSV with the transfer columns.
  const bankLink = page.getByRole("link", { name: "ดาวน์โหลดไฟล์โอน" });
  await expect(bankLink).toBeVisible();
  const bankHref = await bankLink.getAttribute("href");
  expect(bankHref).toContain("bank-payout.csv");
  // Fetch the CSV response (the Next proxy strips the attachment header, so
  // assert the body instead of a download event).
  const csvText = await page.evaluate(async (href) => {
    const res = await fetch(href!);
    return await res.text();
  }, bankHref);
  expect(csvText).toContain("employee_code");
  expect(csvText).toContain("amount");

  // Mark paid (reference prompt), then the run shows completed + reference.
  page.once("dialog", (dialog) => dialog.accept("E2E-REF-1"));
  await page.getByTestId("mark-paid").click();
  await expect(page.getByText("จ่ายแล้ว").first()).toBeVisible();
  await expect(page.getByText("E2E-REF-1")).toBeVisible();

  // The run is already expanded from the anomaly review above; check the payslip
  // (40,000 salary + 4h weekday OT = 41,000).
  await expect(page.getByText("41,000.00").first()).toBeVisible();
  await expect(page.getByRole("link", { name: "PDF" }).first()).toBeVisible();
  await expect(page.getByRole("link", { name: "CSV" }).first()).toBeVisible();
});

test("classifies OT on a company holiday", async ({ page }) => {
  await login(page);
  await page.goto("/attendance");

  await page.getByTestId("holiday-date").fill("2026-10-07");
  await page.getByTestId("holiday-name").fill("ทดสอบวันหยุด");
  await page.getByTestId("add-holiday").click();
  await expect(page.getByText("ทดสอบวันหยุด")).toBeVisible();

  const imported = await apiPost(page, "/api/gw/api/v1/attendance/import", {
    rows: [{ employee_code: "E2E001", date: "2026-10-07", in: "09:00", out: "13:00" }],
  });
  expect(imported.json.imported).toBe(1);

  await page.reload();
  await page.getByTestId("month-picker").fill("2026-10");
  const row = page.getByTestId("day-row").filter({ hasText: "2026-10-07" });
  await expect(row.getByRole("cell", { name: "วันหยุดนักขัตฤกษ์ (3x)" })).toBeVisible();
  await expect(row).toContainText("4:00");

  await page.getByTestId("day-filter").selectOption("incomplete");
  await expect(row).toHaveCount(0);
  await page.getByTestId("day-filter").selectOption("pending");
  await expect(row).toBeVisible();
});

test("invites a user from the admin screen", async ({ page }) => {
  await login(page);
  await page.goto("/users");
  await expect(page.getByRole("heading", { name: "ผู้ใช้", exact: true })).toBeVisible();

  invitedEmail = `invited-${Date.now()}@example.com`;
  await page.getByLabel("อีเมล").fill(invitedEmail);
  await page.getByLabel("ชื่อ (ไทย)").fill("ผู้ถูกเชิญ");
  await page.getByRole("button", { name: "สร้างลิงก์เชิญ" }).click();
  await expect(page.getByText("ลิงก์เชิญ (ใช้ได้ครั้งเดียว")).toBeVisible();
  const inviteLink = page.getByRole("link", { name: /\/invite\// });
  await expect(inviteLink).toBeVisible();
  const inviteHref = await inviteLink.getAttribute("href");
  expect(inviteHref).toBeTruthy();

  // Accept it so the user exists for the reset test.
  await page.goto(inviteHref!);
  const passwords = page.locator('input[type="password"]');
  await passwords.nth(0).fill("InvitedPass123!");
  await passwords.nth(1).fill("InvitedPass123!");
  await page.getByRole("button", { name: "ตั้งรหัสผ่าน" }).click();
  await expect(page.getByRole("heading", { name: "ตั้งรหัสผ่านแล้ว" })).toBeVisible();
});

test("resets the invited user's password with an HR link", async ({ page }) => {
  expect(invitedEmail).not.toBe("");
  await login(page);
  await page.goto("/users");

  const row = page.getByRole("row", { name: new RegExp(invitedEmail) });
  await row.getByRole("button", { name: "รีเซ็ต" }).click();
  const link = page.getByRole("link", { name: /\/reset\// });
  await expect(link).toBeVisible();
  const href = await link.getAttribute("href");
  expect(href).toBeTruthy();

  await page.goto(href!);
  const passwords = page.locator('input[type="password"]');
  await passwords.nth(0).fill("ResetPass123!");
  await passwords.nth(1).fill("ResetPass123!");
  await page.getByRole("button", { name: "ตั้งรหัสผ่าน" }).click();
  await expect(page.getByRole("heading", { name: "ตั้งรหัสผ่านใหม่แล้ว" })).toBeVisible();

  await page.getByRole("button", { name: "ไปหน้าเข้าสู่ระบบ" }).click();
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill(invitedEmail);
  await page.getByLabel("รหัสผ่าน").fill("ResetPass123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
});

test("links an employee account and shows self-service payslips", async ({ page }) => {
  await login(page);
  await page.goto("/employees");
  const row = page.getByRole("row", { name: /E2E001/ });
  await row.getByRole("button", { name: "แก้ไข" }).click();
  await page.getByTestId("employee-cost-center").fill("CC-E2E");
  await page.getByTestId("employee-user").selectOption({ label: `${invitedEmail} · ผู้ถูกเชิญ` });
  await page.getByTestId("save-employee").click();
  await expect(page.getByText("บันทึกการแก้ไขแล้ว")).toBeVisible();

  // Cost center persists across reload.
  await page.reload();
  await page.getByRole("row", { name: /E2E001/ }).getByRole("button", { name: "แก้ไข" }).click();
  await expect(page.getByTestId("employee-cost-center")).toHaveValue("CC-E2E");
  await page.getByRole("button", { name: "ยกเลิก" }).click();

  await page.getByLabel("ออกจากระบบ").click();
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill(invitedEmail);
  await page.getByLabel("รหัสผ่าน").fill("ResetPass123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();

  await expect(page.getByRole("heading", { name: "สลิปของฉัน" })).toBeVisible();
  await expect(page.getByRole("link", { name: "PDF" }).first()).toBeVisible();

  await page.getByTestId("acknowledge-payslip").first().click();
  await expect(page.getByTestId("payslip-ack-status").first()).toContainText("ยืนยันแล้ว");
  await page.reload();
  await expect(page.getByTestId("payslip-ack-status").first()).toContainText("ยืนยันแล้ว");

  const [cert] = await Promise.all([
    page.waitForEvent("popup"),
    page.getByTestId("payslip-50tawi").first().click(),
  ]);
  await expect(cert.getByRole("heading", { name: /หนังสือรับรองการหักภาษี/ })).toBeVisible();
  await cert.close();
});

test("employee punches in and out from the clock page", async ({ page, context }) => {
  await login(page);
  const shift = await apiPost(page, "/api/gw/api/v1/shifts", {
    name: "E2E", start: "09:00", end: "18:00", break_minutes: 60, grace_minutes: 5,
  });
  expect(shift.ok).toBeTruthy();
  const { id: shiftId } = shift.json;
  const list = await apiGet(page, "/api/gw/api/v1/employees?q=E2E001");
  const employeeId = list.json.employees[0].id;
  const assign = await apiPost(page, `/api/gw/api/v1/shifts/${shiftId}/assign`, {
    employee_id: employeeId, effective_from: "2026-01-01",
  });
  expect(assign.ok).toBeTruthy();

  await page.getByLabel("ออกจากระบบ").click();
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill(invitedEmail);
  await page.getByLabel("รหัสผ่าน").fill("ResetPass123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();

  await context.grantPermissions(["geolocation"]);
  await context.setGeolocation({ latitude: 13.7563, longitude: 100.5018 });
  await page.goto("/clock");
  await page.getByRole("button", { name: "เข้างาน" }).click();
  await expect(page.getByText("บันทึกเวลาแล้ว")).toBeVisible();
  await page.getByRole("button", { name: "ออกงาน" }).click();
  await expect(page.getByTestId("day-row").first()).toBeVisible();
});

test("offline punch is queued locally and syncs when back online", async ({ page, context }) => {
  await login(page);
  await page.getByLabel("ออกจากระบบ").click();
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill(invitedEmail);
  await page.getByLabel("รหัสผ่าน").fill("ResetPass123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();

  await context.grantPermissions(["geolocation"]);
  await context.setGeolocation({ latitude: 13.7563, longitude: 100.5018 });
  await page.goto("/clock");

  await context.setOffline(true);
  await page.getByRole("button", { name: "เข้างาน" }).click();
  await expect(page.getByTestId("queue-status")).toContainText("บันทึกในเครื่องแล้ว");

  await context.setOffline(false);
  await expect(page.getByTestId("sync-status")).toContainText("ซิงก์แล้ว");
  await expect(page.getByTestId("day-row").first()).toBeVisible();
});

test("HR creates a shift, corrects and approves a derived day", async ({ page }) => {
  const today = new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(new Date());
  await login(page);
  await page.goto("/attendance/shifts");
  await page.getByTestId("shift-name").fill("E2E UI");
  await page.getByTestId("shift-start").fill("09:00");
  await page.getByTestId("shift-end").fill("18:00");
  await page.getByTestId("save-shift").click();
  await expect(page.getByText("สร้างกะแล้ว")).toBeVisible();

  const employeeId = await page
    .getByTestId("shift-employee")
    .locator("option", { hasText: "E2E001" })
    .getAttribute("value");
  await page.getByTestId("shift-employee").selectOption(employeeId!);
  await page.getByTestId("assign-shift").click();
  await expect(page.getByText("มอบหมายกะแล้ว")).toBeVisible();

  await page.goto("/attendance");
  const row = page.getByTestId("day-row").filter({ hasText: today }).filter({ hasText: "E2E001" });
  await row.getByTestId("correct-day").click();
  await page.getByTestId("correct-in").fill(`${today}T09:00`);
  await page.getByTestId("correct-out").fill(`${today}T18:00`);
  await page.getByTestId("save-correction").click();
  await expect(page.getByText("บันทึกการแก้ไขแล้ว")).toBeVisible();

  await row.getByTestId("approve-day").click();
  await expect(row).toContainText("อนุมัติแล้ว");
});

test("employee requests leave, HR approves it", async ({ page }) => {
  async function loginAs(email: string, password: string) {
    await page.getByLabel("ออกจากระบบ").click();
    await expect(page.getByRole("button", { name: "เข้าสู่ระบบ" })).toBeVisible();
    await page.getByLabel("องค์กร").fill("e2e");
    await page.getByLabel("อีเมล").fill(email);
    await page.getByLabel("รหัสผ่าน").fill(password);
    await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
    await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
  }

  await login(page);
  await loginAs(invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await page.getByTestId("leave-type").selectOption("annual");
  await page.getByTestId("leave-start").fill("2026-12-01");
  await page.getByTestId("leave-end").fill("2026-12-02");
  await page.getByTestId("leave-reason").fill("E2E");
  await page.getByTestId("request-leave").click();
  const mine = page.getByTestId("leave-row").filter({ hasText: "ธ.ค. 2569" });
  await expect(mine).toContainText("รออนุมัติ");

  await loginAs("e2e@example.com", "E2ePassword123!");
  await page.goto("/leave");
  const inbox = page.getByTestId("approval-row").filter({ hasText: "E2E001" }).filter({ hasText: "ธ.ค. 2569" });
  await expect(inbox).toBeVisible();
  await inbox.getByTestId("approve-leave").click();
  await expect(inbox).toHaveCount(0);

  await loginAs(invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await expect(page.getByTestId("leave-row").filter({ hasText: "ธ.ค. 2569" })).toContainText("อนุมัติแล้ว");
});

test("HR bulk-imports employees from a CSV", async ({ page }) => {
  await login(page);
  await page.goto("/employees");
  const csv =
    "employee_code,first_name_th,last_name_th,position_title,department,base_salary,hire_date\n" +
    'E2E-IMP-1,นำเข้า,"ทดสอบ, ซีเอสวี",นักพัฒนา,ไอที,25000.00,2026-10-01\n';
  await page.getByTestId("import-csv").setInputFiles({
    name: "employees.csv",
    mimeType: "text/csv",
    buffer: Buffer.from(csv, "utf-8"),
  });
  await page.getByTestId("dry-run-import").click();
  await expect(page.getByTestId("import-summary")).toContainText("ถูกต้อง 1");
  await page.getByTestId("confirm-import").click();
  await expect(page.getByText("นำเข้าพนักงาน 1 รายการแล้ว")).toBeVisible();
  await expect(page.getByRole("row", { name: /E2E-IMP-1/ })).toBeVisible();

  const bad =
    "employee_code,first_name_th,last_name_th,position_title,base_salary,hire_date\n" +
    "E2E-IMP-BAD,ผิด,พลาด,นักพัฒนา,abc,2026-10-01\n";
  await page.getByTestId("import-csv").setInputFiles({
    name: "bad.csv",
    mimeType: "text/csv",
    buffer: Buffer.from(bad, "utf-8"),
  });
  await page.getByTestId("dry-run-import").click();
  await expect(page.getByTestId("import-summary")).toContainText("ผิดพลาด 1");
  await expect(page.getByTestId("import-row")).toContainText("base_salary ต้องเป็นตัวเลข");
  await expect(page.getByTestId("confirm-import")).toBeDisabled();
  await expect(
    page.locator('tr:not([data-testid="import-row"])').filter({ hasText: "E2E-IMP-BAD" }),
  ).toHaveCount(0);
});

function totp(secret: string): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = "";
  for (const c of secret.replace(/=+$/, "").toUpperCase()) bits += alphabet.indexOf(c).toString(2).padStart(5, "0");
  const key = Buffer.from(bits.match(/.{8}/g)!.map((b) => parseInt(b, 2)));
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(Math.floor(Date.now() / 30000)));
  const h = createHmac("sha1", key).update(counter).digest();
  const o = h[h.length - 1] & 0xf;
  const n = ((h[o] & 0x7f) << 24) | (h[o + 1] << 16) | (h[o + 2] << 8) | h[o + 3];
  return String(n % 1_000_000).padStart(6, "0");
}

test("HR enrolls in 2FA, logs in with a code, then disables it", async ({ page }) => {
  await login(page);
  await page.goto("/settings");
  await page.getByTestId("setup-2fa").click();
  const secret = (await page.getByTestId("totp-secret").innerText()).trim();
  await page.getByTestId("totp-enable-code").fill(totp(secret));
  await page.getByTestId("enable-2fa").click();
  await expect(page.getByTestId("totp-status")).toContainText("เปิดใช้งานอยู่");

  const recoveryCode = (await page.getByTestId("recovery-codes").locator("li").first().innerText()).trim();

  await page.getByLabel("ออกจากระบบ").click();
  await page.goto("/login");
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill("e2e@example.com");
  await page.getByLabel("รหัสผ่าน").fill("E2ePassword123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByTestId("login-totp")).toBeVisible();
  await page.getByTestId("login-totp").fill(totp(secret));
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();

  await page.getByLabel("ออกจากระบบ").click();
  await page.goto("/login");
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill("e2e@example.com");
  await page.getByLabel("รหัสผ่าน").fill("E2ePassword123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByTestId("login-totp")).toBeVisible();
  await page.getByTestId("login-totp").fill(recoveryCode);
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();

  await page.getByLabel("ออกจากระบบ").click();
  await page.goto("/login");
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill("e2e@example.com");
  await page.getByLabel("รหัสผ่าน").fill("E2ePassword123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByTestId("login-totp")).toBeVisible();
  await page.getByTestId("login-totp").fill(recoveryCode);
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toHaveCount(0);

  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill("e2e@example.com");
  await page.getByLabel("รหัสผ่าน").fill("E2ePassword123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByTestId("login-totp")).toBeVisible();
  await page.getByTestId("login-totp").fill(totp(secret));
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();

  await page.goto("/settings");
  await page.getByTestId("totp-disable-code").fill(totp(secret));
  await page.getByTestId("disable-2fa").click();
  await expect(page.getByTestId("totp-status")).toContainText("ปิดอยู่");
});

test("HR saves a LINE id for the invited user", async ({ page }) => {
  expect(invitedEmail).not.toBe("");
  await login(page);
  await page.goto("/users");

  const row = page.getByRole("row", { name: new RegExp(invitedEmail) });
  await row.getByTestId("line-id").fill("U0123456789abcdef0123456789abcdef");
  await row.getByTestId("save-line").click();
  await expect(page.getByText("บันทึก LINE ID แล้ว")).toBeVisible();

  await page.reload();
  await expect(
    page.getByRole("row", { name: new RegExp(invitedEmail) }).getByTestId("line-id"),
  ).toHaveValue("U0123456789abcdef0123456789abcdef");
});

test("employee requests OT, HR approves it", async ({ page }) => {
  async function loginAs(email: string, password: string) {
    await page.getByLabel("ออกจากระบบ").click();
    await expect(page.getByRole("button", { name: "เข้าสู่ระบบ" })).toBeVisible();
    await page.getByLabel("องค์กร").fill("e2e");
    await page.getByLabel("อีเมล").fill(email);
    await page.getByLabel("รหัสผ่าน").fill(password);
    await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
    await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
  }

  await login(page);
  await loginAs(invitedEmail, "ResetPass123!");
  await page.goto("/ot");
  await page.getByTestId("ot-date").fill("2026-12-03");
  await page.getByTestId("ot-hours").fill("2");
  await page.getByTestId("ot-reason").fill("E2E");
  await page.getByTestId("request-ot").click();
  await expect(page.getByTestId("ot-row").filter({ hasText: "3 ธ.ค. 2569" })).toContainText("รออนุมัติ");

  await loginAs("e2e@example.com", "E2ePassword123!");
  await page.goto("/ot");
  const inbox = page.getByTestId("ot-approval-row").filter({ hasText: "E2E001" }).filter({ hasText: "3 ธ.ค. 2569" });
  await expect(inbox).toBeVisible();
  await inbox.getByTestId("approve-ot").click();
  await expect(inbox).toHaveCount(0);
  await expect(
    page.getByTestId("ot-history-row").filter({ hasText: "E2E001" }).filter({ hasText: "3 ธ.ค. 2569" }),
  ).toContainText("อนุมัติแล้ว");

  await loginAs(invitedEmail, "ResetPass123!");
  await page.goto("/ot");
  await expect(page.getByTestId("ot-row").filter({ hasText: "3 ธ.ค. 2569" })).toContainText("อนุมัติแล้ว");
});

test("multi-level leave approval chain", async ({ page }) => {
  async function loginAs(email: string, password: string) {
    await page.getByLabel("ออกจากระบบ").click();
    await expect(page.getByRole("button", { name: "เข้าสู่ระบบ" })).toBeVisible();
    await page.getByLabel("องค์กร").fill("e2e");
    await page.getByLabel("อีเมล").fill(email);
    await page.getByLabel("รหัสผ่าน").fill(password);
    await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
    await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
  }

  await login(page);
  await page.goto("/settings");
  await page.waitForLoadState("networkidle");
  await page.getByTestId("leave-chain").selectOption("manager,hr_admin");
  await page.getByTestId("save-policy").click();
  await expect(page.getByText("บันทึกสูตรเงินเดือนแล้ว")).toBeVisible();

  await loginAs(invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await page.getByTestId("leave-type").selectOption("annual");
  await page.getByTestId("leave-start").fill("2026-12-04");
  await page.getByTestId("leave-end").fill("2026-12-05");
  await page.getByTestId("leave-reason").fill("E2E-2STEP");
  await page.getByTestId("request-leave").click();
  const mine = page.getByTestId("leave-row").filter({ hasText: "E2E-2STEP" });
  await expect(mine).toContainText("รออนุมัติ");

  await loginAs("e2e@example.com", "E2ePassword123!");
  await page.goto("/leave");
  const inbox = page.getByTestId("approval-row").filter({ hasText: "E2E-2STEP" });
  await expect(inbox).toBeVisible();
  await expect(inbox.getByTestId("leave-step")).toContainText("ขั้น 1/2 · หัวหน้างาน");
  await inbox.getByTestId("approve-leave").click();
  await expect(inbox.getByTestId("leave-step")).toContainText("ขั้น 2/2 · HR");
  await inbox.getByTestId("approve-leave").click();
  await expect(inbox).toHaveCount(0);

  await loginAs(invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await expect(page.getByTestId("leave-row").filter({ hasText: "E2E-2STEP" })).toContainText("อนุมัติแล้ว");

  await loginAs("e2e@example.com", "E2ePassword123!");
  await page.goto("/settings");
  await page.waitForLoadState("networkidle");
  await page.getByTestId("leave-chain").selectOption("hr_admin");
  await page.getByTestId("save-policy").click();
  await expect(page.getByText("บันทึกสูตรเงินเดือนแล้ว")).toBeVisible();
});

test("HR opens reports and sees headcount totals", async ({ page }) => {
  await login(page);
  await page.goto("/reports");
  await expect(page.getByRole("heading", { name: "รายงาน" })).toBeVisible();
  await expect(page.getByTestId("headcount-active")).toContainText(/[0-9]+/);
  await expect(page.getByTestId("headcount-probation")).toContainText(/[0-9]+/);
  await expect(page.getByTestId("headcount-leave")).toContainText(/[0-9]+/);

  await page.getByTestId("cost-from").fill("2026-01-01");
  await page.getByTestId("cost-to").fill("2026-12-31");
  await page.getByTestId("cost-load").click();
  await expect(
    page
      .getByRole("table", { name: "ต้นทุนเงินเดือน" })
      .or(page.getByText("ไม่มีข้อมูลต้นทุนในช่วงที่เลือก")),
  ).toBeVisible();
});

test("HR creates a loan for an employee", async ({ page }) => {
  await login(page);
  await page.goto("/loans");
  await page.getByTestId("loan-employee").selectOption({ label: "E2E001 · สมชาย ทดสอบ" });
  await page.getByTestId("loan-principal").fill("10000");
  await page.getByTestId("loan-installment").fill("1000");
  await page.getByTestId("loan-start").fill("2026-12-01");
  await page.getByTestId("create-loan").click();
  await expect(page.getByText("สร้างเงินกู้แล้ว")).toBeVisible();
  const row = page.getByTestId("loan-row").filter({ hasText: "E2E001" });
  await expect(row).toContainText("กำลังผ่อน");
});

test("HR imports opening leave balance and employee sees quota", async ({ page }) => {
  async function loginAs(email: string, password: string) {
    await page.getByLabel("ออกจากระบบ").click();
    await expect(page.getByRole("button", { name: "เข้าสู่ระบบ" })).toBeVisible();
    await page.getByLabel("องค์กร").fill("e2e");
    await page.getByLabel("อีเมล").fill(email);
    await page.getByLabel("รหัสผ่าน").fill(password);
    await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
    await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
  }

  await login(page);
  await page.goto("/leave");

  const csv =
    "employee_code,leave_type,year,total_days,used_days\n" + "E2E001,annual,2026,10,0\n";
  await page.getByTestId("import-balance-csv").setInputFiles({
    name: "balances.csv",
    mimeType: "text/csv",
    buffer: Buffer.from(csv, "utf-8"),
  });
  await page.getByTestId("dry-run-balance-import").click();
  await expect(page.getByTestId("import-summary")).toContainText("ถูกต้อง 1");
  await page.getByTestId("confirm-balance-import").click();
  await expect(page.getByText("นำเข้าโควตาการลา 1 รายการแล้ว")).toBeVisible();

  await loginAs(invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await expect(page.getByText("10 / 10").first()).toBeVisible();
});

test("employee submits an expense claim with receipt and HR approves it", async ({ page }) => {
  async function loginAs(email: string, password: string) {
    await page.getByLabel("ออกจากระบบ").click();
    await expect(page.getByRole("button", { name: "เข้าสู่ระบบ" })).toBeVisible();
    await page.getByLabel("องค์กร").fill("e2e");
    await page.getByLabel("อีเมล").fill(email);
    await page.getByLabel("รหัสผ่าน").fill(password);
    await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
    await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
  }

  // Tiny 1x1 PNG.
  const png = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=",
    "base64",
  );

  await login(page);
  await loginAs(invitedEmail, "ResetPass123!");
  await page.goto("/claims");
  await expect(page.getByRole("heading", { name: "ค่าใช้จ่าย", exact: true })).toBeVisible();

  await page.getByTestId("claim-category").selectOption("meal");
  await page.getByTestId("claim-amount").fill("500");
  await page.getByTestId("claim-date").fill("2026-12-10");
  await page.getByTestId("claim-description").fill("E2E team lunch");
  await page.getByTestId("claim-receipt").setInputFiles({
    name: "receipt.png",
    mimeType: "image/png",
    buffer: png,
  });
  await page.getByTestId("submit-claim").click();
  await expect(page.getByText("ส่งคำขอเบิกค่าใช้จ่ายและแนบใบเสร็จแล้ว")).toBeVisible();

  const mine = page.getByTestId("claim-row").filter({ hasText: "E2E team lunch" });
  await expect(mine).toContainText("รออนุมัติ");
  await expect(mine).toContainText("500.00");
  await expect(mine.getByRole("link", { name: "ดูใบเสร็จ" })).toBeVisible();

  await loginAs("e2e@example.com", "E2ePassword123!");
  await page.goto("/claims");
  const inbox = page.getByTestId("claim-approval-row").filter({ hasText: "E2E001" }).filter({ hasText: "E2E team lunch" });
  await expect(inbox).toBeVisible();
  await expect(inbox.getByRole("link", { name: "ดูใบเสร็จ" })).toBeVisible();
  await inbox.getByTestId("approve-claim").click();
  await expect(inbox).toHaveCount(0);

  await loginAs(invitedEmail, "ResetPass123!");
  await page.goto("/claims");
  await expect(page.getByTestId("claim-row").filter({ hasText: "E2E team lunch" })).toContainText("อนุมัติแล้ว");

  await page.reload();
  await expect(page.getByTestId("claim-row").filter({ hasText: "E2E team lunch" })).toContainText("อนุมัติแล้ว");
});

test("HR computes a final settlement for an employee", async ({ page }) => {
  await login(page);
  await page.goto("/final-settlement");
  await expect(page.getByRole("heading", { name: "พนักงานลาออก" })).toBeVisible();

  await page.getByTestId("settlement-employee").selectOption({ label: "E2E001 · สมชาย ทดสอบ" });
  await page.getByTestId("settlement-date").fill("2026-12-31");
  await page.getByTestId("settlement-unused").fill("5");
  await page.getByTestId("settlement-notice").fill("30");
  await page.getByTestId("compute-settlement").click();

  await expect(page.getByText("คำนวณเงินค่าจ้างแล้ว")).toBeVisible();
  await expect(page.getByTestId("settlement-total")).toContainText(/[0-9,]+\.\d{2}/);

  // Stored as a draft in the list.
  const row = page.getByTestId("settlement-row").filter({ hasText: "E2E001" });
  await expect(row).toBeVisible();
  await expect(row).toContainText("ฉบับร่าง");

  // No payroll run is created.
  await page.goto("/payroll");
  await expect(page.getByTestId("create-run")).toBeVisible();
});

test("HR opens settings and sees the notification summary without error", async ({ page }) => {
  await login(page);
  await page.goto("/settings");
  await expect(page.getByRole("heading", { name: "การแจ้งเตือน" })).toBeVisible();
  await expect(page.getByTestId("notify-summary")).toContainText(/รอส่ง/);
  await expect(page.getByTestId("notify-summary")).toContainText(/ล้มเหลว/);
  await expect(page.getByTestId("notify-summary")).toContainText(/ส่งแล้ว/);

  const empty = page.getByTestId("notify-empty");
  const rows = page.getByTestId("notification-row");
  await expect(empty.or(rows.first())).toBeVisible();
});

async function switchUser(page: Page, email: string, password: string) {
  await page.getByLabel("ออกจากระบบ").click();
  await page.goto("/login");
  await expect(page.getByRole("heading", { name: "เข้าสู่ระบบ" })).toBeVisible();
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill(email);
  await page.getByLabel("รหัสผ่าน").fill(password);
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
}

test("rejects a wrong password and stays on the login page", async ({ page }) => {
  await page.goto("/login");
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill("e2e@example.com");
  await page.getByLabel("รหัสผ่าน").fill("definitely-wrong-password");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.locator(".error")).toBeVisible();
  await expect(page.getByRole("heading", { name: "เข้าสู่ระบบ" })).toBeVisible();
});

test("employee navigation hides HR/admin areas", async ({ page }) => {
  await login(page);
  await switchUser(page, invitedEmail, "ResetPass123!");
  await expect(page.getByRole("link", { name: "ลงเวลา" })).toBeVisible();
  await expect(page.getByRole("link", { name: "พนักงาน" })).toHaveCount(0);
  await expect(page.getByRole("link", { name: "ตั้งค่า" })).toHaveCount(0);

  await page.goto("/employees");
  await expect(page).toHaveURL(/\/clock$/);
  await expect(page.getByTestId("add-employee")).toHaveCount(0);
  await page.goto("/settings");
  await expect(page).toHaveURL(/\/clock$/);
});

test("leave rejects an inverted date range", async ({ page }) => {
  await login(page);
  await switchUser(page, invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await page.getByTestId("leave-type").selectOption("annual");
  await page.getByTestId("leave-start").fill("2026-12-10");
  await page.getByTestId("leave-end").fill("2026-12-09");
  await page.getByTestId("leave-reason").fill("E2E-BAD-RANGE");
  await page.getByTestId("request-leave").click();
  await expect(page.locator(".error")).toBeVisible();
  await expect(page.getByTestId("leave-row").filter({ hasText: "E2E-BAD-RANGE" })).toHaveCount(0);
});

test("employee cancels a pending leave request", async ({ page }) => {
  await login(page);
  await switchUser(page, invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await page.getByTestId("leave-type").selectOption("annual");
  await page.getByTestId("leave-start").fill("2026-12-11");
  await page.getByTestId("leave-end").fill("2026-12-12");
  await page.getByTestId("leave-reason").fill("E2E-CANCEL");
  await page.getByTestId("request-leave").click();
  const row = page.getByTestId("leave-row").filter({ hasText: "E2E-CANCEL" });
  await expect(row).toContainText("รออนุมัติ");
  await row.getByTestId("cancel-leave").click();
  await expect(row).toContainText("ยกเลิกแล้ว");

  await login(page);
  await page.goto("/leave");
  await expect(page.getByTestId("approval-row").filter({ hasText: "E2E-CANCEL" })).toHaveCount(0);
});

test("HR rejects a leave request and the employee sees it", async ({ page }) => {
  await login(page);
  await switchUser(page, invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await page.getByTestId("leave-type").selectOption("annual");
  await page.getByTestId("leave-start").fill("2026-12-13");
  await page.getByTestId("leave-end").fill("2026-12-14");
  await page.getByTestId("leave-reason").fill("E2E-REJECT");
  await page.getByTestId("request-leave").click();
  await expect(page.getByTestId("leave-row").filter({ hasText: "E2E-REJECT" })).toContainText("รออนุมัติ");

  await login(page);
  await page.goto("/leave");
  const inbox = page.getByTestId("approval-row").filter({ hasText: "E2E-REJECT" });
  await expect(inbox).toBeVisible();
  await inbox.getByTestId("reject-leave").click();
  await expect(inbox).toHaveCount(0);

  await switchUser(page, invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await expect(page.getByTestId("leave-row").filter({ hasText: "E2E-REJECT" })).toContainText("ไม่อนุมัติ");
});

test("employee cancels OT and HR rejects another request", async ({ page }) => {
  await login(page);
  await switchUser(page, invitedEmail, "ResetPass123!");
  await page.goto("/ot");
  await page.getByTestId("ot-date").fill("2026-12-15");
  await page.getByTestId("ot-hours").fill("1");
  await page.getByTestId("ot-reason").fill("E2E-OT-CANCEL");
  await page.getByTestId("request-ot").click();
  const row = page.getByTestId("ot-row").filter({ hasText: "E2E-OT-CANCEL" });
  await expect(row).toContainText("รออนุมัติ");
  await row.getByTestId("cancel-ot").click();
  await expect(row).toContainText("ยกเลิกแล้ว");

  await page.getByTestId("ot-date").fill("2026-12-16");
  await page.getByTestId("ot-hours").fill("1");
  await page.getByTestId("ot-reason").fill("E2E-OT-REJECT");
  await page.getByTestId("request-ot").click();

  await login(page);
  await page.goto("/ot");
  const inbox = page.getByTestId("ot-approval-row").filter({ hasText: "E2E-OT-REJECT" });
  await expect(inbox).toBeVisible();
  await inbox.getByTestId("reject-ot").click();
  await expect(inbox).toHaveCount(0);

  await switchUser(page, invitedEmail, "ResetPass123!");
  await page.goto("/ot");
  await expect(page.getByTestId("ot-row").filter({ hasText: "E2E-OT-REJECT" })).toContainText("ไม่อนุมัติ");
});

test("employee cost center and PF rate persist", async ({ page }) => {
  await login(page);
  await page.goto("/employees");
  let row = page.getByRole("row", { name: /E2E001/ });
  await row.getByRole("button", { name: "แก้ไข" }).click();
  await page.getByTestId("employee-cost-center").fill("CC-E2E");
  await page.getByLabel("เป็นสมาชิกกองทุนสำรองเลี้ยงชีพ").check();
  await page.getByTestId("employee-pf-rate").fill("3");
  await page.getByTestId("employee-pf-rate-from").fill("2026-01-01");
  await page.getByTestId("save-employee").click();
  await expect(page.getByText("บันทึกการแก้ไขแล้ว")).toBeVisible();

  await page.reload();
  row = page.getByRole("row", { name: /E2E001/ });
  await row.getByRole("button", { name: "แก้ไข" }).click();
  await expect(page.getByTestId("employee-cost-center")).toHaveValue("CC-E2E");
  await expect(page.getByTestId("employee-pf-rate")).toHaveValue("3");
});

test("bulk import dry-run blocks an invalid file", async ({ page }) => {
  await login(page);
  await page.goto("/employees");
  const csv = [
    "employee_code,first_name_th,last_name_th,position_title,department,base_salary,hire_date",
    "E2E-BAD,ทดสอบ,ทดสอบ,Dev,IT,not-a-number,2024-01-01",
  ].join("\n");
  await page.getByTestId("import-csv").setInputFiles({
    name: "bad.csv",
    mimeType: "text/csv",
    buffer: Buffer.from(csv),
  });
  await page.getByTestId("dry-run-import").click();
  await expect(page.getByTestId("import-summary")).toContainText("ผิดพลาด");
  await expect(page.getByTestId("confirm-import")).toBeDisabled();
});

test("form template edits persist", async ({ page }) => {
  await login(page);
  await page.goto("/forms");
  await page.getByTestId("form-type").selectOption("50tawi");
  await page.getByTestId("form-title").fill("50 ทวิ (E2E)");
  await page.getByTestId("form-save").click();
  await expect(page.getByText("บันทึกแบบฟอร์มแล้ว")).toBeVisible();
  await page.reload();
  await page.getByTestId("form-type").selectOption("50tawi");
  await expect(page.getByTestId("form-title")).toHaveValue("50 ทวิ (E2E)");

  await page.getByTestId("form-title").fill("หนังสือรับรองการหักภาษี ณ ที่จ่าย");
  await page.getByTestId("form-save").click();
});

test("audit page lists login events", async ({ page }) => {
  await login(page);
  await page.goto("/audit");
  await expect(page.getByText("auth.login").first()).toBeVisible();
});

test("final settlement rejects a termination before hire", async ({ page }) => {
  await login(page);
  await page.goto("/final-settlement");
  const employeeSelect = page.getByTestId("settlement-employee");
  const employeeValue = await employeeSelect
    .locator("option")
    .filter({ hasText: "E2E001" })
    .getAttribute("value");
  await employeeSelect.selectOption(employeeValue ?? "");
  await page.getByTestId("settlement-date").fill("2020-01-01");
  await page.getByTestId("settlement-unused").fill("0");
  await page.getByTestId("settlement-notice").fill("0");
  await page.getByTestId("compute-settlement").click();
  await expect(page.locator(".error")).toBeVisible();
  await expect(page.getByTestId("settlement-total")).toHaveCount(0);
});

test("reports shows an empty state for an empty period", async ({ page }) => {
  await login(page);
  await page.goto("/reports");
  await page.getByTestId("cost-from").fill("1990-01-01");
  await page.getByTestId("cost-to").fill("1990-12-31");
  await page.getByTestId("cost-load").click();
  await expect(page.getByText("ไม่มีข้อมูลต้นทุนในช่วงที่เลือก")).toBeVisible();
});

test("2FA rejects a wrong enrollment code", async ({ page }) => {
  await login(page);
  await page.goto("/settings");
  await page.getByTestId("setup-2fa").click();
  await expect(page.getByTestId("totp-secret")).toBeVisible();
  await page.getByTestId("totp-enable-code").fill("000000");
  await page.getByTestId("enable-2fa").click();
  await expect(page.locator(".error")).toBeVisible();
  await expect(page.getByTestId("disable-2fa")).toHaveCount(0);
});

test("session management lists and revokes all sessions", async ({ page }) => {
  await login(page);
  await page.goto("/settings");
  await expect(page.getByTestId("session-row").first()).toBeVisible();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByTestId("revoke-all-sessions").click();
  await expect(page.getByTestId("revoke-all-sessions")).toBeEnabled();
});


test("manager role sees team areas but no admin or payroll", async ({ page }) => {
  await login(page);
  await page.goto("/users");
  await page.getByLabel("อีเมล").fill(managerEmail);
  await page.getByLabel("ชื่อ (ไทย)").fill("ผู้จัดการ E2E");
  await page.getByLabel("บทบาท", { exact: true }).selectOption("manager");
  await page.getByRole("button", { name: "สร้างลิงก์เชิญ" }).click();
  const link = page.getByRole("link", { name: /\/invite\// });
  const href = await link.getAttribute("href");
  expect(href).toBeTruthy();
  await page.goto(href!);
  const passwords = page.locator('input[type="password"]');
  await passwords.nth(0).fill("ManagerPass123!");
  await passwords.nth(1).fill("ManagerPass123!");
  await page.getByRole("button", { name: "ตั้งรหัสผ่าน" }).click();
  await expect(page.getByRole("heading", { name: "ตั้งรหัสผ่านแล้ว" })).toBeVisible();
  await page.getByRole("button", { name: "ไปหน้าเข้าสู่ระบบ" }).click();
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill(managerEmail);
  await page.getByLabel("รหัสผ่าน").fill("ManagerPass123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();

  await expect(page.getByRole("link", { name: "เวลาและ OT" })).toBeVisible();
  await expect(page.getByRole("link", { name: "การลา" })).toBeVisible();
  await expect(page.getByRole("link", { name: "ค่าใช้จ่าย" })).toBeVisible();
  await expect(page.getByRole("link", { name: "รอบเงินเดือน", exact: true })).toHaveCount(0);
  await expect(page.getByRole("link", { name: "ตั้งค่า" })).toHaveCount(0);

  await page.goto("/payroll");
  await expect(page).toHaveURL(/\/clock$/);
  await page.goto("/settings");
  await expect(page).toHaveURL(/\/clock$/);
});

test("payroll manager role sees payroll/reports but no admin or loans", async ({ page }) => {
  await login(page);
  await page.goto("/users");
  const managerRow = page.getByRole("row", { name: /manager-e2e@example.com/ });
  await managerRow
    .getByLabel(`เพิ่มบทบาทให้ ${managerEmail}`)
    .selectOption("payroll_manager");
  await expect(page.getByText(/เพิ่มบทบาท payroll_manager/)).toBeVisible();

  await switchUser(page, managerEmail, "ManagerPass123!");
  await expect(page.getByRole("link", { name: "รอบเงินเดือน", exact: true })).toBeVisible();
  await expect(page.getByRole("link", { name: "รายงาน" })).toBeVisible();
  await expect(page.getByRole("link", { name: "ตั้งค่า" })).toHaveCount(0);
  await expect(page.getByRole("link", { name: "เงินกู้" })).toHaveCount(0);

  await page.goto("/settings");
  await expect(page).toHaveURL(/\/clock$/);
  await page.goto("/loans");
  await expect(page).toHaveURL(/\/clock$/);
});

test("manager approves step one, HR approves step two", async ({ page }) => {
  await login(page);
  await page.goto("/settings");
  await page.waitForLoadState("networkidle");
  await page.getByTestId("leave-chain").selectOption("manager,hr_admin");
  await page.getByTestId("save-policy").click();
  await expect(page.getByText("บันทึกสูตรเงินเดือนแล้ว")).toBeVisible();

  await switchUser(page, invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await page.getByTestId("leave-type").selectOption("annual");
  await page.getByTestId("leave-start").fill("2026-12-20");
  await page.getByTestId("leave-end").fill("2026-12-21");
  await page.getByTestId("leave-reason").fill("E2E-MANAGER");
  await page.getByTestId("request-leave").click();
  await expect(page.getByTestId("leave-row").filter({ hasText: "E2E-MANAGER" })).toContainText("รออนุมัติ");

  await switchUser(page, managerEmail, "ManagerPass123!");
  await page.goto("/leave");
  const inbox = page.getByTestId("approval-row").filter({ hasText: "E2E-MANAGER" });
  await expect(inbox.getByTestId("leave-step")).toContainText("ขั้น 1/2");
  await inbox.getByTestId("approve-leave").click();
  await expect(inbox.getByTestId("leave-step")).toContainText("ขั้น 2/2");

  await switchUser(page, "e2e@example.com", "E2ePassword123!");
  await page.goto("/leave");
  const hrInbox = page.getByTestId("approval-row").filter({ hasText: "E2E-MANAGER" });
  await expect(hrInbox.getByTestId("leave-step")).toContainText("ขั้น 2/2");
  await hrInbox.getByTestId("approve-leave").click();
  await expect(hrInbox).toHaveCount(0);

  await switchUser(page, invitedEmail, "ResetPass123!");
  await page.goto("/leave");
  await expect(page.getByTestId("leave-row").filter({ hasText: "E2E-MANAGER" })).toContainText("อนุมัติแล้ว");

  await switchUser(page, "e2e@example.com", "E2ePassword123!");
  await page.goto("/settings");
  await page.waitForLoadState("networkidle");
  await page.getByTestId("leave-chain").selectOption("hr_admin");
  await page.getByTestId("save-policy").click();
  await expect(page.getByText("บันทึกสูตรเงินเดือนแล้ว")).toBeVisible();
});

test("HR creates a holiday and sees it on the team calendar", async ({ page }) => {
  await login(page);
  await page.goto("/attendance");
  await page.getByTestId("holiday-date").fill("2026-11-25");
  await page.getByTestId("holiday-name").fill("วันลอยกระทง");
  await page.getByTestId("add-holiday").click();
  await expect(page.getByText("วันลอยกระทง")).toBeVisible();

  await page.goto("/calendar");
  await page.getByTestId("calendar-month").fill("2026-11");
  const day = page.getByTestId("calendar-day").filter({ hasText: "วันลอยกระทง" });
  await expect(day).toBeVisible();
  await expect(day).toContainText("25 พ.ย. 2569");
});

test("employee accepts the privacy notice and exports their personal data", async ({ page }) => {
  await login(page);
  await page.getByLabel("ออกจากระบบ").click();
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill(invitedEmail);
  await page.getByLabel("รหัสผ่าน").fill("ResetPass123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();

  await page.goto("/privacy");
  await expect(page.getByRole("heading", { name: /ความเป็นส่วนตัว/ })).toBeVisible();
  await page.getByTestId("accept-consent").click();
  await expect(page.getByTestId("consent-status")).toContainText("รับทราบประกาศความเป็นส่วนตัว");

  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByTestId("export-data").click(),
  ]);
  const data = JSON.parse(readFileSync((await download.path())!, "utf8"));
  expect(data.employees[0].employee_code).toBe("E2E001");
  expect(data.account.email).toBe(invitedEmail);
  expect(data.notice_version).toBeTruthy();
  expect(Array.isArray(data.employees[0].payslips)).toBe(true);
});

test("HR adds an expiring document and downloads its file", async ({ page }) => {
  await login(page);
  await page.goto("/documents");

  const employeeId = await page
    .getByTestId("doc-employee")
    .locator("option", { hasText: "E2E001" })
    .getAttribute("value");
  await page.getByTestId("doc-employee").selectOption(employeeId!);
  await page.getByTestId("doc-type").selectOption("work_permit");
  await page.getByTestId("doc-title").fill("ใบอนุญาตทำงาน E2E");
  const expiry = new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(
    new Date(Date.now() + 20 * 86_400_000),
  );
  await page.getByTestId("doc-expiry").fill(expiry);
  await page.getByTestId("doc-file").setInputFiles({
    name: "permit.pdf",
    mimeType: "application/pdf",
    buffer: Buffer.from("%PDF-1.4 e2e permit"),
  });
  await page.getByTestId("save-document").click();
  await expect(page.getByText("เพิ่มเอกสารแล้ว")).toBeVisible();

  const row = page.getByTestId("document-row").filter({ hasText: "ใบอนุญาตทำงาน E2E" });
  await expect(row).toBeVisible();
  await expect(page.getByTestId("expiring-list")).toContainText("ใบอนุญาตทำงาน E2E");

  const href = await row.getByTestId("doc-download").getAttribute("href");
  const res = await page.request.get(href!);
  expect(res.status()).toBe(200);
  expect(await res.text()).toContain("%PDF-1.4 e2e permit");
});

test("HR runs the standard onboarding checklist", async ({ page }) => {
  await login(page);
  await page.goto("/onboarding");

  const employeeId = await page
    .getByTestId("onboarding-employee")
    .locator("option", { hasText: "E2E001" })
    .getAttribute("value");
  await page.getByTestId("onboarding-employee").selectOption(employeeId!);
  await page.getByTestId("standard-checklist").click();
  await expect(page.getByText("สร้างรายการมาตรฐานแล้ว")).toBeVisible();

  await expect(page.getByTestId("onboarding-task")).toHaveCount(6);
  await expect(page.getByTestId("onboarding-progress")).toContainText("เสร็จ 0/6");
  await page.getByTestId("task-done").first().click();
  await expect(page.getByTestId("onboarding-progress")).toContainText("เสร็จ 1/6");
  await expect(page.getByTestId("onboarding-task").last()).toContainText("เสร็จแล้ว");
});

test("HR opens a job and moves a candidate through the pipeline", async ({ page }) => {
  await login(page);
  await page.goto("/recruitment");

  await page.getByTestId("job-title").fill("นักพัฒนาระบบ E2E");
  await page.getByTestId("job-department").fill("Engineering");
  await page.getByTestId("job-headcount").fill("2");
  await page.getByTestId("create-job").click();
  await expect(page.getByText("เปิดรับสมัครแล้ว")).toBeVisible();

  const jobRow = page.getByTestId("job-row").filter({ hasText: "นักพัฒนาระบบ E2E" });
  await expect(jobRow).toBeVisible();
  await jobRow.getByTestId("select-job").click();

  await page.getByTestId("cand-first").fill("สมหญิง");
  await page.getByTestId("cand-last").fill("ทดสอบ");
  await page.getByTestId("cand-email").fill("candidate@example.com");
  await page.getByTestId("cand-phone").fill("0800000000");
  await page.getByTestId("cand-source").fill("referral");
  await page.getByTestId("add-candidate").click();
  await expect(page.getByText("เพิ่มผู้สมัครแล้ว")).toBeVisible();

  const candidate = page.getByTestId("candidate-row").filter({ hasText: "สมหญิง" });
  await expect(candidate).toBeVisible();
  await candidate.getByTestId("candidate-stage").selectOption("interview");
  await expect(candidate.getByTestId("candidate-stage")).toHaveValue("interview");
  await expect(jobRow).toContainText("1 คน");
});

test("HR records an expiring training certificate", async ({ page }) => {
  await login(page);
  await page.goto("/training");

  const employeeId = await page
    .getByTestId("training-employee")
    .locator("option", { hasText: "E2E001" })
    .getAttribute("value");
  await page.getByTestId("training-employee").selectOption(employeeId!);
  await page.getByTestId("training-title").fill("ความปลอดภัย E2E");
  await page.getByTestId("training-provider").fill("TalentOS Academy");
  const today = new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(new Date());
  const expiry = new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Bangkok" }).format(
    new Date(Date.now() + 30 * 86_400_000),
  );
  await page.getByTestId("training-completed").fill(today);
  await page.getByTestId("training-expires").fill(expiry);
  await page.getByTestId("save-training").click();
  await expect(page.getByText("บันทึกการอบรมแล้ว")).toBeVisible();

  await expect(page.getByTestId("training-row").filter({ hasText: "ความปลอดภัย E2E" })).toBeVisible();
  await expect(page.getByTestId("training-expiring")).toContainText("ความปลอดภัย E2E");
});
