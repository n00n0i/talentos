import { expect, Page, test } from "@playwright/test";

test.describe.configure({ mode: "serial" });

const TOKEN = process.env.E2E_SETUP_TOKEN ?? "e2e-setup-token";
const RUN = Date.now().toString(36);
const SLUG = `roles-${RUN}`;
const ADMIN = `roles-admin-${RUN}@example.com`;
const MANAGER = `roles-manager-${RUN}@example.com`;
const PAYROLL = `roles-payroll-${RUN}@example.com`;
const EMPLOYEE = `roles-employee-${RUN}@example.com`;
const PASSWORD = "RolesPass123!";

async function login(page: Page, email: string, slug = SLUG) {
  await page.goto("/login");
  await page.getByLabel("องค์กร").fill(slug);
  await page.getByLabel("อีเมล").fill(email);
  await page.getByLabel("รหัสผ่าน").fill(PASSWORD);
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
}

async function accept(page: Page, href: string) {
  await page.goto(href);
  const pw = page.locator('input[type="password"]');
  await pw.nth(0).fill(PASSWORD);
  await pw.nth(1).fill(PASSWORD);
  await page.getByRole("button", { name: "ตั้งรหัสผ่าน" }).click();
  await expect(page.getByRole("heading", { name: "ตั้งรหัสผ่านแล้ว" })).toBeVisible();
}

test.beforeAll(async ({ browser }) => {
  const page = await browser.newPage({ baseURL: process.env.E2E_BASE_URL ?? "http://127.0.0.1:3100" });
  await page.goto("/setup");
  await page.getByTestId("setup-org").fill(`Roles ${RUN}`);
  await page.getByTestId("setup-slug").fill(SLUG);
  await page.getByTestId("setup-entity").fill("Roles Main");
  await page.getByTestId("setup-email").fill(ADMIN);
  await page.getByTestId("setup-password").fill(PASSWORD);
  await page.getByTestId("setup-confirm").fill(PASSWORD);
  await page.getByTestId("setup-token").fill(TOKEN);
  await page.getByTestId("setup-submit").click();
  await expect(page.getByRole("heading", { name: "เข้าสู่ระบบ" })).toBeVisible();

  await login(page, ADMIN);
  for (const [email, role] of [
    [MANAGER, "manager"],
    [PAYROLL, "payroll_manager"],
    [EMPLOYEE, "employee"],
  ] as const) {
    await page.goto("/users");
    await page.getByLabel("อีเมล").fill(email);
    await page.getByLabel("ชื่อ (ไทย)").fill(`Role ${role}`);
    await page.getByLabel("บทบาท", { exact: true }).selectOption(role);
    await page.getByRole("button", { name: "สร้างลิงก์เชิญ" }).click();
    const href = await page.getByRole("link", { name: /\/invite\// }).getAttribute("href");
    expect(href).toBeTruthy();
    await accept(page, href!);
    await login(page, ADMIN);
  }
  await page.close();
});

test("HR admin can open every admin module", async ({ page }) => {
  await login(page, ADMIN);
  for (const path of [
    "/employees",
    "/attendance",
    "/payroll",
    "/reports",
    "/settings",
    "/users",
    "/forms",
    "/audit",
    "/loans",
    "/final-settlement",
  ]) {
    await page.goto(path);
    await expect(page).toHaveURL(new RegExp(`${path.replace("/", "\\/")}$`));
  }
});

test("payroll manager is limited to payroll, reports, people", async ({ page }) => {
  await login(page, PAYROLL);
  for (const path of ["/payroll", "/reports", "/employees"]) {
    await page.goto(path);
    await expect(page).toHaveURL(new RegExp(`${path.replace("/", "\\/")}$`));
  }
  for (const path of ["/settings", "/users", "/forms", "/audit", "/loans", "/final-settlement"]) {
    await page.goto(path);
    await expect(page).toHaveURL(/\/clock$/);
  }
});

test("manager is limited to team and self-service areas", async ({ page }) => {
  await login(page, MANAGER);
  for (const path of ["/attendance", "/leave", "/ot", "/claims", "/clock"]) {
    await page.goto(path);
    await expect(page).toHaveURL(new RegExp(`${path.replace("/", "\\/")}$`));
  }
  for (const path of [
    "/employees",
    "/payroll",
    "/reports",
    "/settings",
    "/users",
    "/forms",
    "/audit",
    "/loans",
    "/final-settlement",
  ]) {
    await page.goto(path);
    await expect(page).toHaveURL(/\/clock$/);
  }
});

test("employee is limited to self-service areas", async ({ page }) => {
  await login(page, EMPLOYEE);
  for (const path of ["/leave", "/ot", "/claims", "/clock", "/me/payslips"]) {
    await page.goto(path);
    await expect(page).toHaveURL(new RegExp(`${path.replace("/", "\\/")}$`));
  }
  for (const path of [
    "/employees",
    "/attendance",
    "/calendar",
    "/payroll",
    "/reports",
    "/settings",
    "/users",
    "/forms",
    "/audit",
    "/loans",
    "/final-settlement",
  ]) {
    await page.goto(path);
    await expect(page).toHaveURL(/\/clock$/);
  }
});

test("employee cannot open calendar; manager can", async ({ page }) => {
  await login(page, EMPLOYEE);
  await expect(page.getByRole("link", { name: "ปฏิทินทีม" })).toHaveCount(0);
  await page.goto("/calendar");
  await expect(page).toHaveURL(/\/clock$/);

  await page.getByLabel("ออกจากระบบ").click();
  await login(page, MANAGER);
  await expect(page.getByRole("link", { name: "ปฏิทินทีม" })).toBeVisible();
  await page.goto("/calendar");
  await expect(page).toHaveURL(/\/calendar$/);
});
