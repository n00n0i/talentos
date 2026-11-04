import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
const OUT = new URL("../evidence/prototype/", import.meta.url).pathname;
const URL = new URL("../prototype/index.html", import.meta.url).href;
mkdirSync(OUT, { recursive: true });
const b = await chromium.launch({ args: ["--no-sandbox"] });
const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, recordVideo: { dir: OUT, size: { width: 1280, height: 720 } } });
const p = await ctx.newPage();
await p.goto(URL);
await p.screenshot({ path: `${OUT}/01-overview-desktop.png` });
for (const [view, n] of [["assistant", "02-assistant"], ["studio", "03-studio-connectors"]]) {
  await p.locator(`#nav [data-view="${view}"]`).click();
  await p.waitForTimeout(350);
  await p.screenshot({ path: `${OUT}/${n}.png` });
}
for (const [tab, n] of [["connect", "04-connect"], ["blueprint", "05-blueprint"], ["flow", "06-flow"], ["monitor", "07-monitor"], ["approval", "08-approval"]]) {
  await p.locator(`#studiotabs [data-tab="${tab}"]`).click();
  await p.waitForTimeout(300);
  await p.screenshot({ path: `${OUT}/${n}.png` });
}
await p.setViewportSize({ width: 390, height: 844 });
await p.locator(`#mobilebar [data-view="overview"]`).click();
await p.screenshot({ path: `${OUT}/09-overview-mobile.png` });
await p.locator(`#mobilebar [data-view="studio"]`).click();
await p.screenshot({ path: `${OUT}/10-studio-mobile.png` });
await ctx.close();
await b.close();
