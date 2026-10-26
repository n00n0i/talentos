import { expect, Page, test } from "@playwright/test";

test.describe.configure({ mode: "serial" });

async function login(page: Page) {
  await page.goto("/login");
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill("e2e@example.com");
  await page.getByLabel("รหัสผ่าน").fill("E2ePassword123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByRole("link", { name: "ภาพรวม" })).toBeVisible();
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

  await page.goto("/attendance");
  await page.getByTestId("attendance-date").fill("2026-10-05");
  await page.getByTestId("attendance-ot").fill("4");
  await page.getByTestId("save-attendance").click();
  await expect(page.getByText("บันทึกเวลาแล้ว")).toBeVisible();
  await expect(page.getByRole("cell", { name: "2026-10-05" })).toBeVisible();

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
});

test("invites a user from the admin screen", async ({ page }) => {
  await login(page);
  await page.goto("/users");
  await expect(page.getByRole("heading", { name: "ผู้ใช้", exact: true })).toBeVisible();

  const email = `invited-${Date.now()}@example.com`;
  await page.getByLabel("อีเมล").fill(email);
  await page.getByLabel("ชื่อ (ไทย)").fill("ผู้ถูกเชิญ");
  await page.getByRole("button", { name: "สร้างลิงก์เชิญ" }).click();
  await expect(page.getByText("ลิงก์เชิญ (ใช้ได้ครั้งเดียว")).toBeVisible();
  await expect(page.getByText(`/invite/`).first()).toBeVisible();
});
