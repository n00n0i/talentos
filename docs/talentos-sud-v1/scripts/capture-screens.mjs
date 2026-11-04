import { chromium } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";
const BASE = process.env.E2E_BASE_URL || "http://127.0.0.1:3100";
const OUT = "/home/opc/content-creator/talentos/docs/talentos-sud-v1/evidence/screens";
mkdirSync(OUT, { recursive: true });
const browser = await chromium.launch({ args: ["--no-sandbox"] });
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const page = await ctx.newPage();
let errors = [];
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
page.on("pageerror", (e) => errors.push("PAGEERROR " + e.message));
async function login(email, pass) {
  await page.goto(BASE + "/login");
  await page.getByLabel("องค์กร").fill("e2e");
  await page.getByLabel("อีเมล").fill(email);
  await page.getByLabel("รหัสผ่าน").fill(pass);
  await page.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
  await page.getByLabel("ออกจากระบบ").waitFor({ timeout: 15000 });
}
async function logout() { await page.getByLabel("ออกจากระบบ").click(); await page.waitForURL(/\/login/).catch(()=>{}); }
async function shot(role, route, width, h, name) {
  errors = [];
  await page.setViewportSize({ width, height: h });
  await page.goto(BASE + route, { waitUntil: "networkidle" });
  await page.waitForTimeout(500);
  const data = await page.evaluate(() => ({
    h1: [...document.querySelectorAll("h1")].map((e) => e.textContent?.trim()),
    overflow: document.documentElement.scrollWidth - innerWidth,
    labels: [...document.querySelectorAll("table.table td[data-label]")].length,
  }));
  const slug = `${role}/${width}${route.replace(/[^a-z0-9]+/gi, "_") || "_root"}`;
  await page.screenshot({ path: `${OUT}/${slug}.png`, fullPage: false });
  return { role, route, width, ...data, console: [...new Set(errors)], file: `${slug}.png` };
}
const results = [];
await login("e2e@example.com", "E2ePassword123!");
const users = await page.evaluate(async () => (await fetch("/api/gw/api/v1/users")).json());
const invited = users.users?.find((u) => String(u.email).startsWith("invited-"));
const adminRoutes = ["/","/employees","/team","/documents","/onboarding","/recruitment","/training","/performance","/goals","/attendance","/attendance/shifts","/calendar","/clock","/leave","/ot","/claims","/payroll","/loans","/final-settlement","/reports","/payslip","/me/payslips","/me/reviews","/privacy","/users","/settings","/settings/ai","/settings/ai/graph","/forms","/audit"];
for (const r of adminRoutes) results.push(await shot("admin", r, 1440, 900));
for (const r of ["/","/employees","/team","/users","/payroll","/recruitment","/performance","/goals","/attendance","/settings"]) {
  for (const w of [390, 760, 1024]) results.push(await shot("admin", r, w, w === 390 ? 844 : w === 760 ? 900 : 900));
}
// EN evidence
await page.setViewportSize({ width: 1440, height: 900 });
await page.goto(BASE + "/", { waitUntil: "networkidle" });
await page.getByTestId("lang-toggle").click();
for (const r of ["/", "/claims", "/recruitment", "/team", "/settings"]) results.push(await shot("en", r, 1440, 900));
await ctx.clearCookies();
await logout().catch(()=>{});
await login(invited.email, "ResetPass123!");
results.push(await shot("employee", "/", 1440, 900));
for (const r of ["/clock", "/leave", "/ot", "/claims", "/me/payslips", "/me/reviews", "/privacy"]) results.push(await shot("employee", r, 1440, 900));
for (const r of ["/clock", "/leave", "/me/reviews"]) results.push(await shot("employee", r, 390, 844));
await logout().catch(()=>{});
await login("manager-e2e@example.com", "ManagerPass123!");
results.push(await shot("manager", "/", 1440, 900));
for (const r of ["/team", "/performance", "/goals", "/attendance"]) results.push(await shot("manager", r, 1440, 900));
results.push(await shot("manager", "/team", 390, 844));
writeFileSync("/home/opc/content-creator/talentos/docs/talentos-sud-v1/evidence/responsive/checks.json", JSON.stringify(results, null, 2));
console.log("captured", results.length, "screens; failures", results.filter(r=>r.overflow>1 || r.console.length || r.h1.length!==1).length);
await browser.close();
