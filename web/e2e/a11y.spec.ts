import AxeBuilder from "@axe-core/playwright";
import { expect, Page, test } from "@playwright/test";

test.describe.configure({ mode: "serial" });

const TOKEN = process.env.E2E_SETUP_TOKEN ?? "e2e-setup-token";
const RUN = Date.now().toString(36);
const SLUG = `a11y-${RUN}`;
const ADMIN = `a11y-admin-${RUN}@example.com`;
const PASSWORD = "A11yPass12345!";

const ROUTES = [
  "/",
  "/employees",
  "/team",
  "/payroll",
  "/reports",
  "/documents",
  "/recruitment",
  "/attendance",
  "/attendance/shifts",
  "/leave",
  "/ot",
  "/claims",
  "/clock",
  "/me/payslips",
  "/me/reviews",
  "/privacy",
  "/users",
  "/settings",
  "/settings/ai",
  "/settings/ai/graph",
  "/forms",
  "/audit",
];

async function login(page: Page) {
  await page.goto("/login");
  await page.getByLabel("องค์กร").fill(SLUG);
  await page.getByLabel("อีเมล").fill(ADMIN);
  await page.getByLabel("รหัสผ่าน").fill(PASSWORD);
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
}

test.beforeAll(async ({ browser }) => {
  const page = await browser.newPage({ baseURL: process.env.E2E_BASE_URL ?? "http://127.0.0.1:3100" });
  await page.goto("/setup");
  await page.getByTestId("setup-org").fill(`A11y ${RUN}`);
  await page.getByTestId("setup-slug").fill(SLUG);
  await page.getByTestId("setup-entity").fill("A11y Main");
  await page.getByTestId("setup-email").fill(ADMIN);
  await page.getByTestId("setup-password").fill(PASSWORD);
  await page.getByTestId("setup-confirm").fill(PASSWORD);
  await page.getByTestId("setup-token").fill(TOKEN);
  await page.getByTestId("setup-submit").click();
  await expect(page.getByRole("heading", { name: "เข้าสู่ระบบ" })).toBeVisible();
  await page.close();
});

for (const width of [390, 760, 1440]) {
  test(`axe WCAG 2.1 AA and target sizing at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await login(page);
    const violations: unknown[] = [];
    for (const route of ROUTES) {
      await page.goto(route, { waitUntil: "networkidle" });
      const result = await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
        .analyze();
      for (const v of result.violations) {
        if (v.impact === "critical" || v.impact === "serious") {
          violations.push({ route, id: v.id, impact: v.impact, nodes: v.nodes.map((n) => n.target) });
        }
      }
      const controls = await page.evaluate(() => {
        const visible = (e: Element) => {
          const r = e.getBoundingClientRect();
          const s = getComputedStyle(e);
          return (
            r.width > 0 &&
            r.height > 0 &&
            s.display !== "none" &&
            s.visibility !== "hidden" &&
            e.getAttribute("aria-hidden") !== "true"
          );
        };
        const unlabeled = [...document.querySelectorAll("input,select,textarea")]
          .filter(visible)
          .filter(
            (e) =>
              !e.getAttribute("aria-label") &&
              !e.getAttribute("placeholder") &&
              !(e.id && document.querySelector(`label[for="${e.id}"]`)) &&
              !e.closest("label"),
          );
        const small = [...document.querySelectorAll("button,a")]
          .filter(visible)
          .filter((e) => {
            const r = e.getBoundingClientRect();
            return r.width < 36 || r.height < 36;
          });
        return { unlabeled: unlabeled.length, small: small.length };
      });
      expect(controls.unlabeled, `${route} unlabeled controls`).toBe(0);
      expect(controls.small, `${route} targets under 36px`).toBe(0);
    }
    expect(violations, JSON.stringify(violations, null, 2)).toEqual([]);
  });
}

test("axe WCAG 2.1 AA in English mode", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => {
    if (m.type() === "error" && /418|hydrat/i.test(m.text())) errors.push(m.text());
  });
  await login(page);
  await page.getByTestId("lang-toggle").click();
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  const violations: unknown[] = [];
  for (const route of ["/", "/employees", "/payroll", "/settings", "/leave", "/team"]) {
    await page.goto(route, { waitUntil: "networkidle" });
    const result = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      .analyze();
    for (const v of result.violations) {
      if (v.impact === "critical" || v.impact === "serious") {
        violations.push({ route, id: v.id, nodes: v.nodes.map((n) => n.target) });
      }
    }
  }
  expect(violations, JSON.stringify(violations, null, 2)).toEqual([]);
  expect(errors, `EN runtime errors: ${errors.join(" | ")}`).toEqual([]);
});
