const { chromium } = require(
  process.env.ISLAND_PLAYWRIGHT_PATH || "playwright",
);
const assert = require("node:assert/strict");
const fs = require("node:fs");
const output = process.env.ISLAND_UI_OUTPUT || "/tmp/island-map-ux";
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
      reducedMotion: "reduce",
    });
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    // Observe the selected marker's actual draw position, without app debug hooks.
    await page.addInitScript(() => {
      const arc = CanvasRenderingContext2D.prototype.arc;
      CanvasRenderingContext2D.prototype.arc = function (x, y, r, ...rest) {
        if (r === 12) window.selectedPin = { x, y };
        return arc.call(this, x, y, r, ...rest);
      };
    });
    await page.goto(process.env.ISLAND_URL || "http://127.0.0.1:7453");
    await page.waitForFunction(() => document.body.dataset.ready === "true");
    const idle = () =>
      page.waitForFunction(() => !document.body.classList.contains("busy"));
    const frame = () =>
      page.evaluate(
        () =>
          new Promise((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(resolve)),
          ),
      );
    const scale = () =>
      page.evaluate(() => {
        const label = document.querySelector("#scale-label").textContent;
        return (
          parseFloat(document.querySelector("#scale-line").style.width) /
          (parseFloat(label) * (label.includes("km") ? 1000 : 1))
        );
      });
    assert.equal(
      await page.locator("#connection, #clock, .topbar, .console-bar").count(),
      0,
    );
    assert.equal(await page.locator("[role=tabpanel]:visible").count(), 1);
    assert.equal(await page.locator("#pane-explore").isVisible(), true);
    assert.deepEqual(await page.locator("#plat").boundingBox(), {
      x: 0,
      y: 0,
      width: 1440,
      height: 1000,
    });
    await page.screenshot({ path: `${output}/explore.png` });
    const initialScale = await scale();
    await page.getByRole("button", { name: /Bryant Park park/ }).click();
    await frame();
    assert.equal(await page.locator("#place-card").isVisible(), true);
    assert.equal(await page.locator("dialog:modal").count(), 0);
    assert.equal(await page.locator("[role=tabpanel]:visible").count(), 0);
    assert.ok((await scale()) > initialScale * 2, "Selecting a place zooms in");
    let panel = await page.locator("#activity-panel").boundingBox();
    let pin = await page.evaluate(() => window.selectedPin);
    assert.ok(
      pin.x > panel.x + panel.width + 24,
      "Selected place remains beside the panel",
    );
    await page.screenshot({ path: `${output}/place-card.png` });
    // The card leaves the map interactive, and clicking the map marker reopens it.
    await page.keyboard.press("Escape");
    await frame();
    await page.mouse.click(pin.x, pin.y);
    await frame();
    assert.equal(await page.locator("#place-card").isVisible(), true);
    assert.equal(
      await page.locator("#place-title").textContent(),
      "Bryant Park",
    );
    pin = await page.evaluate(() => window.selectedPin);
    await page.mouse.move(1100, 650);
    await page.mouse.down();
    await page.mouse.move(1200, 680, { steps: 8 });
    await page.mouse.up();
    await frame();
    const moved = await page.evaluate(() => window.selectedPin);
    assert.ok(Math.abs(moved.x - pin.x) > 80, "Map pans with the card open");
    assert.equal(await page.locator("#place-card").isVisible(), true);
    await page.locator("#place-dismiss").click();
    await page.locator("#place-search").fill("Wash Sq subway");
    await page.getByRole("button", { name: /W 4 St-Wash Sq/ }).click();
    await page.locator("#subway-explore").click();
    await page.locator("#subway-neighbors .place-result").first().waitFor();
    const original = await page.locator("#place-title").textContent();
    await page.locator("#subway-neighbors .place-result").first().click();
    assert.notEqual(await page.locator("#place-title").textContent(), original);
    await page.locator("#place-back").click();
    assert.equal(await page.locator("#place-title").textContent(), original);
    await page.locator("#place-back").click();
    assert.equal(
      await page.locator("#place-search").inputValue(),
      "Wash Sq subway",
    );
    assert.equal(await page.locator("#place-card").isVisible(), false);
    await page.getByRole("button", { name: /W 4 St-Wash Sq/ }).click();
    await page.locator("#place-route").click();
    await idle();
    assert.equal(await page.locator("#pane-route").isVisible(), true);
    assert.equal(await page.locator("#place-card").isVisible(), false);
    assert.match(await page.locator("#to-search").inputValue(), /W 4 St/);
    await page.screenshot({ path: `${output}/directions.png` });
    await page.locator("#nav-route").focus();
    await page.keyboard.press("ArrowRight");
    assert.equal(
      await page.locator("#nav-missions").getAttribute("aria-selected"),
      "true",
    );
    assert.equal(await page.locator("#pane-missions").isVisible(), true);
    await page.keyboard.press("ArrowRight");
    assert.equal(await page.locator("#pane-closures").isVisible(), true);
    await page.locator("#btn-pick-streets").click();
    await page.locator("#nav-explore").click();
    assert.equal(
      await page.locator("#btn-pick-streets").getAttribute("aria-pressed"),
      "false",
    );
    await page.locator("#panel-toggle").click();
    assert.equal(await page.locator("#panel-content").isVisible(), false);
    assert.ok(
      (await page.locator("#activity-panel").boundingBox()).height < 150,
    );
    await page.locator("#nav-explore").click();
    await page.locator("#place-search").fill("Bryant");
    await page.getByRole("button", { name: /Bryant Park park/ }).click();
    await page.setViewportSize({ width: 390, height: 844 });
    await frame();
    panel = await page.locator("#activity-panel").boundingBox();
    pin = await page.evaluate(() => window.selectedPin);
    assert.ok(
      pin.y < panel.y - 15 && pin.y > 20,
      "Mobile place remains above the sheet",
    );
    assert.ok(
      panel.height <= 844 * 0.49,
      "Map retains more than half the mobile screen",
    );
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollHeight > innerHeight,
      ),
      false,
    );
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
      false,
    );
    assert.equal(await page.locator("dialog:modal").count(), 0);
    await page.screenshot({ path: `${output}/mobile-place.png` });
    await page.locator("#btn-theme").click();
    await frame();
    await page.screenshot({ path: `${output}/mobile-dark.png` });
    await page.setViewportSize({ width: 1440, height: 1000 });
    await frame();
    await page.screenshot({ path: `${output}/desktop-dark.png` });
    assert.deepEqual(errors, []);
    console.log(
      "Map UX passed: modes, keyboard, zoom, unobstructed marker, map selection/panning, card stack, directions, collapse, mobile and dark theme",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
