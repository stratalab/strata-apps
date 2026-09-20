const { chromium } = require(
  process.env.ISLAND_PLAYWRIGHT_PATH || "playwright",
);
const assert = require("node:assert/strict");
const path = require("node:path"),
  fs = require("node:fs");
const output =
  process.env.ISLAND_UI_OUTPUT ||
  path.join(require("node:os").tmpdir(), "island-ui-check");
fs.mkdirSync(output, { recursive: true });
(async () => {
  const browser = await chromium.launch({
    executablePath: process.env.ISLAND_CHROMIUM,
    headless: true,
    args: ["--no-sandbox"],
  });
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1000 },
  });
  page.setDefaultTimeout(120000); // Full graph forks are measured separately; allow completion here.
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(process.env.ISLAND_URL || "http://127.0.0.1:7453");
  const meta = await page.evaluate(() =>
    fetch("/api/meta").then((r) => r.json()),
  );
  assert.equal(
    meta.durable,
    false,
    "Browser mutation checks require a disposable cache-mode server",
  );
  await page.waitForFunction(
    () => document.querySelector("#route-distance").textContent !== "—",
  );
  await page.waitForFunction(() => document.body.dataset.ready === "true");
  assert.equal(
    await page.locator("#place-count").textContent(),
    `${meta.place_count} PLACES`,
  );
  await page.screenshot({ path: path.join(output, "v2-initial.png") });
  await page.locator("#place-search").fill("Bryant");
  await page.getByRole("button", { name: /Bryant Park park/ }).click();
  await page.locator("#place-more > summary").click();
  await page.locator("#place-explore").click();
  await page.locator(".relationship-svg").waitFor();
  await page.screenshot({ path: path.join(output, "v2-details.png") });
  await page.locator("#place-dismiss").click();
  await page.locator("#nav-route").click();
  await page.locator("#to-search").fill("Manhattan Bridge Arch");
  await page.locator("#to-search").press("ArrowDown");
  await page.locator("#to-search").press("Enter");
  assert.match(
    await page.locator("#to-search").inputValue(),
    /Manhattan Bridge Arch/,
  );
  await page.locator("#btn-route").click();
  await page.waitForFunction(() => !document.body.classList.contains("busy"));
  await page.locator("#nav-explore").click();
  await page.locator(".discovery-options > summary").click();
  await page.locator("#btn-discover").click();
  await page.waitForFunction(() =>
    document.querySelector("#places-status").textContent.includes("reachable"),
  );
  await page.locator("#nav-closures").click();
  await page.locator("#closure-picker").selectOption("42nd");
  await page.locator("#btn-close").click();
  await page.waitForFunction(() => !document.body.classList.contains("busy"));
  await page.locator("#btn-impact").click();
  await page.waitForFunction(() =>
    document.querySelector("#places-status").textContent.includes("affected"),
  );
  console.log("Impact", await page.locator("#places-status").textContent());
  await page.locator("#nav-closures").click();
  await page.locator("#btn-reopen").click();
  await page.waitForFunction(() => !document.body.classList.contains("busy"));
  assert.equal(await page.locator("#closure-tag").textContent(), "REOPENED");
  await page.screenshot({ path: path.join(output, "v2-desktop.png") });
  await page.locator("#btn-theme").click();
  await page.screenshot({ path: path.join(output, "v2-dark.png") });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: path.join(output, "v2-mobile.png"),
    fullPage: true,
  });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
  );
  assert.deepEqual(errors, []);
  console.log("Browser flows passed");
  await browser.close();
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
