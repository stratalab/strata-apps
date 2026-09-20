const { chromium } = require(
  process.env.ISLAND_PLAYWRIGHT_PATH || "playwright",
);
const assert = require("node:assert/strict");
const fs = require("node:fs");
const output = process.env.ISLAND_UI_OUTPUT || "/tmp/island-subway-ui";
fs.mkdirSync(output, { recursive: true });
(async () => {
  const browser = await chromium.launch({
    executablePath: process.env.ISLAND_CHROMIUM,
    headless: true,
    args: ["--no-sandbox"],
  });
  try {
    const page = await browser.newPage({
      viewport: { width: 1440, height: 1000 },
    });
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(process.env.ISLAND_URL || "http://127.0.0.1:7453");
    await page.waitForFunction(() => document.body.dataset.ready === "true");
    await page.locator("#nav-layers").click();
    assert.match(
      await page.locator("#subway-summary").textContent(),
      /151 stations · 121 complexes · 842/,
    );
    await page.locator("#subway-line").selectOption("1");
    await page.screenshot({ path: `${output}/subway-line.png` });
    await page.locator("#show-subway").uncheck();
    await page.locator("#show-subway").check();
    await page.locator("#subway-line").selectOption("");
    await page.locator("#nav-layers").click();
    await page.locator('[data-category="transit"]').click();
    await page.locator("#place-search").fill("subway");
    await page.waitForFunction(
      () =>
        document.querySelector("#places-status").textContent === "151 places",
    );
    await page.locator("#place-search").fill("Wash Sq subway");
    await page
      .locator("#place-results")
      .getByRole("button", { name: /W 4 St-Wash Sq/ })
      .click();
    assert.match(
      await page.locator("#place-title").textContent(),
      /W 4 St-Wash Sq/,
    );
    assert.equal(
      await page.locator("#subway-services .subway-service").count(),
      7,
    );
    assert.match(await page.locator("#place-source").textContent(), /MTA/);
    await page.locator("#subway-explore").click();
    await page.waitForFunction(() =>
      document
        .querySelector("#subway-explore-status")
        .textContent.includes("stations within"),
    );
    assert.ok(
      (await page.locator("#subway-neighbors .place-result").count()) > 0,
    );
    await page.screenshot({ path: `${output}/station-exploration.png` });
    await page.locator("#place-dismiss").click();
    await page.locator("#place-search").fill("Roosevelt Island subway");
    await page
      .locator("#place-results")
      .getByRole("button", { name: /Roosevelt Island/ })
      .click();
    assert.equal(await page.locator("#place-route").isDisabled(), true);
    assert.equal(await page.locator("#subway-explore").isDisabled(), false);
    await page.locator("#subway-explore").click();
    await page.waitForFunction(() =>
      document
        .querySelector("#subway-explore-status")
        .textContent.includes("stations within"),
    );
    assert.equal(await page.locator("#place-card").isVisible(), true);
    await page.locator("#place-dismiss").click();
    await page.locator("#nav-route").click();
    await page.locator("#to-search").fill("Times Sq subway");
    await page.locator("#to-search").press("ArrowDown");
    await page.locator("#to-search").press("Enter");
    assert.match(await page.locator("#to-search").inputValue(), /Times Sq/);
    await page.locator("#btn-route").click();
    await page.waitForFunction(() => !document.body.classList.contains("busy"));
    await page.screenshot({ path: `${output}/subway-network.png` });
    await page.locator("#btn-theme").click();
    await page.screenshot({ path: `${output}/subway-dark.png` });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.locator("#nav-layers").click();
    await page.screenshot({
      path: `${output}/subway-mobile.png`,
      fullPage: true,
    });
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
      false,
    );
    assert.deepEqual(errors, []);
    console.log(
      "Subway browser checks passed: catalog, service filter, BFS, unanchored station, street endpoint, dark/mobile",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
