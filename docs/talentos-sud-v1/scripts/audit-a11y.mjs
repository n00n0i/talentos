import { chromium } from "@playwright/test";
import { writeFileSync } from "node:fs";
const BASE = process.env.E2E_BASE_URL || "http://127.0.0.1:3100";
const OUT = new URL("../evidence/responsive/", import.meta.url).pathname;
const routes = ["/","/employees","/users","/payroll","/team","/claims","/recruitment","/settings"];
const b = await chromium.launch({ args: ["--no-sandbox"] });
const p = await b.newPage({ viewport: { width: 1440, height: 900 } });
await p.goto(BASE + "/login");
await p.getByLabel("องค์กร").fill("e2e");
await p.getByLabel("อีเมล").fill("e2e@example.com");
await p.getByLabel("รหัสผ่าน").fill("E2ePassword123!");
await p.getByRole("button", { name: "เข้าสู่ระบบ" }).click();
await p.getByLabel("ออกจากระบบ").waitFor();
const checks = [];
const small = new Set();
const unlabeled = new Set();
for (const width of [320, 390, 760, 1024, 1440]) {
  await p.setViewportSize({ width, height: 900 });
  for (const route of routes) {
    await p.goto(BASE + route, { waitUntil: "networkidle" });
    await p.waitForTimeout(200);
    const d = await p.evaluate(() => {
      const visible = (e) => { const r = e.getBoundingClientRect(), s = getComputedStyle(e); return r.width > 0 && r.height > 0 && s.display !== "none" && s.visibility !== "hidden" && e.getAttribute("aria-hidden") !== "true"; };
      const small = [...document.querySelectorAll("button,a")].filter(visible).filter((e) => { const r = e.getBoundingClientRect(); return r.width < 36 || r.height < 36; });
      const inputs = [...document.querySelectorAll("input,select,textarea")].filter(visible);
      const unlabeled = inputs.filter((e) => !e.getAttribute("aria-label") && !e.getAttribute("placeholder") && !(e.id && document.querySelector(`label[for="${e.id}"]`)) && !e.closest("label"));
      return {
        overflow: document.documentElement.scrollWidth - innerWidth,
        h1: document.querySelectorAll("h1").length,
        small: small.length,
        smallKeys: small.map((e) => `${e.tagName}|${e.className}|${(e.textContent || e.getAttribute("aria-label") || "").trim().slice(0, 40)}`),
        unlabeled: unlabeled.length,
        unlabeledKeys: unlabeled.map((e) => `${e.tagName}|${e.type}|${e.getAttribute("data-testid") || ""}|${e.className}`),
      };
    });
    d.smallKeys.forEach((k) => small.add(k));
    d.unlabeledKeys.forEach((k) => unlabeled.add(k));
    checks.push({ width, route, overflow: d.overflow, h1: d.h1, small: d.small, unlabeled: d.unlabeled });
  }
}
writeFileSync(`${OUT}/a11y-unique.json`, JSON.stringify({ checks, uniqueSmallTargets: small.size, uniqueUnlabeledInputs: unlabeled.size, unlabeledSamples: [...unlabeled] }, null, 2));
console.log("uniqueSmall", small.size, "uniqueUnlabeled", unlabeled.size);
await b.close();
