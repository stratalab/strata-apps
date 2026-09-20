const { chromium } = require(
  process.env.ISLAND_PLAYWRIGHT_PATH || "playwright",
);
const assert = require("node:assert/strict");
const fs = require("node:fs");
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
    const base = process.env.ISLAND_URL || "http://127.0.0.1:7453";
    await page.goto(base);
    await page.waitForFunction(() => document.body.dataset.ready === "true");
    await page.locator("#place-search").fill("230 W 55th St");
    await page
      .locator("#place-results")
      .getByRole("button", { name: /230 West 55 Street/ })
      .click();
    assert.match(
      await page.locator("#place-title").textContent(),
      /230 West 55 Street/,
    );
    assert.equal(await page.locator("#place-card").isVisible(), true);
    assert.equal(await page.locator("#place-route").isDisabled(), false);
    await page.locator("#address-stations").click();
    await page.locator("#address-neighbors .place-result").first().waitFor();
    assert.match(
      await page.locator("#address-neighbors").textContent(),
      /by street/,
    );
    await page.locator("#address-discover").click();
    await page.waitForFunction(() =>
      document
        .querySelector("#address-neighbors")
        .textContent.includes("within 1 km"),
    );
    await page.locator("#place-more").click();
    await page.locator("#place-explore").click();
    await page.locator("#relationship-view svg").waitFor();
    assert.match(await page.locator("#place-source").textContent(), /NYC/);
    const out = process.env.ISLAND_UI_OUTPUT || "/tmp/island-address-ui";
    fs.mkdirSync(out, { recursive: true });
    await page.screenshot({ path: `${out}/address-card.png` });
    await page.locator("#place-route").click();
    await page.waitForFunction(() => !document.body.classList.contains("busy"));
    assert.match(
      await page.locator("#to-search").inputValue(),
      /230 West 55 Street/,
    );
    await page.locator("#from-search").fill("270 E 2 Street");
    await page
      .locator("#from-suggestions")
      .getByRole("option", { name: /270 East 2 Street/ })
      .click();
    await page.locator("#btn-route").click();
    await page.waitForFunction(() => !document.body.classList.contains("busy"));
    assert.notEqual(await page.locator("#route-distance").textContent(), "—");
    await page.screenshot({ path: `${out}/address-route.png` });
    await page.locator("#nav-explore").click();
    await page.locator("#place-search").fill("350 fif");
    await page
      .locator("#place-results")
      .getByRole("button", { name: /350 5 Avenue/ })
      .click();
    assert.match(
      await page.locator("#place-title").textContent(),
      /350 5 Avenue/,
    );
    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
      false,
    );
    await page.screenshot({ path: `${out}/address-mobile.png` });
    await page.locator("#btn-theme").click();
    await page.screenshot({ path: `${out}/address-dark.png` });
    await page.locator("#place-dismiss").click();
    await page.locator("#place-search").fill("350 5 Avenue apt 4");
    await page.waitForFunction(() =>
      document
        .querySelector("#places-status")
        .textContent.includes("without an apartment"),
    );
    assert.deepEqual(errors, []);
    console.log(
      "Address browser checks passed: unified search, aliases, cards, graph relationships, station/radius search, route endpoints, units, mobile/dark.",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
