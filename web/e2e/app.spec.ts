import { expect, Page, test } from "@playwright/test";

test.describe.configure({ mode: "serial" });

let invitedEmail = "";

async function login(page: Page) {
  await page.goto("/login");
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill("e2e@example.com");
  await page.getByLabel("รหัสผ่าน").fill("E2ePassword123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
}

test("accepts the invite and logs in", async ({ page }) => {
  await page.goto("/invite/e2e-invite-token-4f8a2c1d9b7e6a3f5c0d8e2b1a9f7c6d");
  const passwords = page.locator('input[type="password"]');
  await passwords.nth(0).fill("E2ePassword123!");
  await passwords.nth(1).fill("E2ePassword123!");
  await page.getByRole("button", { name: "ตั้งรหัสผ่าน" }).click();
  await expect(page.getByRole("heading", { name: "ตั้งรหัสผ่านแล้ว" })).toBeVisible();

  await page.getByRole("button", { name: "ไปหน้าเข้าสู่ระบบ" }).click();
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
  const shift = await page.request.post("/api/gw/api/v1/shifts", {
    data: { name: "E2E OT", start: "09:00", end: "18:00", break_minutes: 60, grace_minutes: 0 },
  });
  expect(shift.ok()).toBeTruthy();
  const { id: shiftId } = await shift.json();
  const found = await page.request.get("/api/gw/api/v1/employees?q=E2E001");
  const employeeId = (await found.json()).employees[0].id;
  const assign = await page.request.post(`/api/gw/api/v1/shifts/${shiftId}/assign`, {
    data: { employee_id: employeeId, effective_from: "2026-01-01" },
  });
  expect(assign.ok()).toBeTruthy();
  const imported = await page.request.post("/api/gw/api/v1/attendance/import", {
    data: { rows: [{ employee_code: "E2E001", date: "2026-10-05", in: "09:00", out: "22:00" }] },
  });
  expect((await imported.json()).imported).toBe(1);

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

  await page.getByTestId("approve-run").click();
  await expect(page.getByText("อนุมัติรอบเงินเดือนแล้ว")).toBeVisible();
  await expect(page.getByText("อนุมัติแล้ว").first()).toBeVisible();

  // Expand the run and check the payslip (40,000 salary + 4h weekday OT = 41,000).
  await page.getByRole("cell", { name: /–/ }).first().click();
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

  const imported = await page.request.post("/api/gw/api/v1/attendance/import", {
    data: { rows: [{ employee_code: "E2E001", date: "2026-10-07", in: "09:00", out: "13:00" }] },
  });
  expect((await imported.json()).imported).toBe(1);

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
  await page.getByTestId("employee-user").selectOption({ label: `${invitedEmail} · ผู้ถูกเชิญ` });
  await page.getByTestId("save-employee").click();
  await expect(page.getByText("บันทึกการแก้ไขแล้ว")).toBeVisible();

  await page.getByLabel("ออกจากระบบ").click();
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill(invitedEmail);
  await page.getByLabel("รหัสผ่าน").fill("ResetPass123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();

  await expect(page.getByRole("heading", { name: "สลิปของฉัน" })).toBeVisible();
  await expect(page.getByRole("link", { name: "PDF" }).first()).toBeVisible();
});

test("employee punches in and out from the clock page", async ({ page, context }) => {
  await login(page);
  const shift = await page.request.post("/api/gw/api/v1/shifts", {
    data: { name: "E2E", start: "09:00", end: "18:00", break_minutes: 60, grace_minutes: 5 },
  });
  expect(shift.ok()).toBeTruthy();
  const { id: shiftId } = await shift.json();
  const list = await page.request.get("/api/gw/api/v1/employees?q=E2E001");
  const employeeId = (await list.json()).employees[0].id;
  const assign = await page.request.post(`/api/gw/api/v1/shifts/${shiftId}/assign`, {
    data: { employee_id: employeeId, effective_from: "2026-01-01" },
  });
  expect(assign.ok()).toBeTruthy();

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
