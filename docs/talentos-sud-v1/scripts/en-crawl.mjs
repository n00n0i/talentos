import { chromium } from "@playwright/test";
import { writeFileSync } from "node:fs";
const BASE = process.env.E2E_BASE_URL || "http://127.0.0.1:3100";
const OUT = new URL("../evidence/", import.meta.url).pathname;
const routes = ["/","/employees","/team","/documents","/onboarding","/recruitment","/training","/performance","/goals","/attendance","/attendance/shifts","/calendar","/clock","/leave","/ot","/claims","/payroll","/loans","/final-settlement","/reports","/payslip","/me/payslips","/me/reviews","/privacy","/users","/settings","/settings/ai","/settings/ai/graph","/forms","/audit"];
const b = await chromium.launch({ args: ["--no-sandbox"] });
const p = await b.newPage({ viewport: { width: 1440, height: 900 } });
await p.goto(BASE + "/login");
await p.getByLabel("องค์กร").fill("e2e");
await p.getByLabel("อีเมล").fill("e2e@example.com");
await p.getByLabel("รหัสผ่าน").fill("E2ePassword123!");
await p.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
await p.getByLabel("ออกจากระบบ").waitFor();
await p.getByTestId("lang-toggle").click();
const out = [];
for (const route of routes) {
  await p.goto(BASE + route, { waitUntil: "networkidle" });
  await p.waitForTimeout(600);
  const d = await p.evaluate(() => ({
    h1: [...document.querySelectorAll("h1")].map((e) => e.textContent?.trim()),
    thai: (document.body.innerText.match(/[\u0E00-\u0E7F]{3,}/g) || []).slice(0, 8),
    overflow: document.documentElement.scrollWidth - innerWidth,
    lang: document.documentElement.lang,
  }));
  const slug = route.replace(/[^a-z0-9]+/gi, "_") || "_root";
  await p.screenshot({ path: `${OUT}/screens/en/1440${slug}.png` });
  out.push({ route, ...d });
}
writeFileSync(`${OUT}/responsive/en-routes.json`, JSON.stringify(out, null, 2));
console.log(out.filter((x) => x.overflow > 1 || x.h1.length !== 1 || x.lang !== "en").length, "bad of", out.length);
await b.close();
