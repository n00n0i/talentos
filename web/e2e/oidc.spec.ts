import { expect, Page, test } from "@playwright/test";

test.describe.configure({ mode: "serial" });

async function login(page: Page) {
  await page.goto("/login");
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill("e2e@example.com");
  await page.getByLabel("รหัสผ่าน").fill("E2ePassword123!");
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await expect(page.getByLabel("ออกจากระบบ")).toBeVisible();
}

test("OIDC settings persist and show masked secret on reload", async ({ page }) => {
  await login(page);
  await page.goto("/settings");
  await expect(page.getByTestId("oidc-settings")).toBeVisible();

  const issuer = "https://sso.example.com";
  const clientId = "talentos-e2e-client";
  const clientSecret = "e2e-secret-value-xyz";

  await page.getByTestId("oidc-issuer").fill(issuer);
  await page.getByTestId("oidc-client-id").fill(clientId);
  await page.getByTestId("oidc-client-secret").fill(clientSecret);
  await page.getByTestId("oidc-allowed-domains").fill("example.com, corp.example.com");
  await page.getByTestId("oidc-enabled").check();

  await page.getByTestId("oidc-save").click();
  await expect(page.getByText("บันทึกตังคาส SSO / OIDC แลว")).toBeVisible();

  // The status badge should show "configured".
  await expect(page.getByTestId("oidc-status")).toContainText("ตังคาสแลว");

  // Reload and verify the settings persist.
  await page.reload();
  await expect(page.getByTestId("oidc-settings")).toBeVisible();
  await expect(page.getByTestId("oidc-issuer")).toHaveValue(issuer);
  await expect(page.getByTestId("oidc-client-id")).toHaveValue(clientId);
  await expect(page.getByTestId("oidc-allowed-domains")).toHaveValue("example.com, corp.example.com");

  // The secret must be masked, not the full value.
  const secretVal = await page.getByTestId("oidc-client-secret").inputValue();
  expect(secretVal).not.toContain(clientSecret);

  // The status badge should still show "configured".
  await expect(page.getByTestId("oidc-status")).toContainText("ตังคาสแลว");
});
