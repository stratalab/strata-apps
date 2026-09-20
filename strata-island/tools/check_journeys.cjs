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
    await page.goto(process.env.ISLAND_URL || "http://127.0.0.1:7454");
    await page.waitForFunction(() => document.body.dataset.ready === "true");
    const idle = () =>
      page.waitForFunction(() => !document.body.classList.contains("busy"));
    await page.locator("#nav-route").click();
    await page.locator("#mode-transit").click();
    await idle();
    assert.equal(
      await page.locator("#mode-transit").getAttribute("aria-pressed"),
      "true",
    );
    async function endpoint(which, query, name) {
      await page.locator("#" + which + "-search").fill(query);
      await page
        .locator("#" + which + "-suggestions")
        .getByRole("option", { name, exact: true })
        .click();
    }
    await endpoint("from", "South Ferry", "South Ferry · 1 subway");
    await endpoint("to", "Roosevelt Island", "Roosevelt Island · M subway");
    const response = page.waitForResponse(
      (r) => r.url().endsWith("/api/route") && r.request().method() === "POST",
    );
    await page.locator("#btn-route").click();
    const data = await (await response).json();
    await idle();
    assert.equal(data.mode, "transit");
    assert.ok(data.boardings >= 1);
    assert.ok(data.legs.some((l) => l.mode === "subway"));
    assert.equal(data.transfers, data.boardings - 1);
    assert.match(
      await page.locator("#journey-note").textContent(),
      /No live arrivals/,
    );
    assert.equal(await page.locator("#route-unit").textContent(), "min est.");
    assert.ok(await page.locator("#journey-legs li").count());
    fs.mkdirSync("/tmp/island-journey-ui", { recursive: true });
    await page.screenshot({ path: "/tmp/island-journey-ui/transit.png" });
    await page.locator("#journey-legs button").last().click();
    await idle();
    // Addresses are usable in the same planner, then mode switching restores car output.
    await endpoint("from", "230 W 55th St", "230 West 55 Street");
    await endpoint("to", "350 fif", "350 5 Avenue");
    await page.locator("#btn-route").click();
    await idle();
    assert.equal(await page.locator("#route-unit").textContent(), "min est.");
    await page.locator("#mode-car").click();
    await idle();
    assert.equal(await page.locator("#journey-legs").isVisible(), false);
    assert.notEqual(
      await page.locator("#route-unit").textContent(),
      "min est.",
    );
    await page.locator("#mode-transit").click();
    await idle();
    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
      false,
    );
    await page.screenshot({ path: "/tmp/island-journey-ui/mobile.png" });
    await page.locator("#btn-theme").click();
    await page.screenshot({ path: "/tmp/island-journey-ui/dark.png" });
    assert.deepEqual(errors, []);
    console.log(
      "Journey browser checks passed: modes, train itinerary, unanchored-car station, address endpoints, mode switching, step camera, mobile/dark.",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
