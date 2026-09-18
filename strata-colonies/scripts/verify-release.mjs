import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium, firefox, webkit } from "playwright";
import AxeBuilder from "@axe-core/playwright";
import { serve } from "./serve-browser.mjs";
const local = process.env.COLONIES_SITE_URL ? null : await serve(0);
const url = process.env.COLONIES_SITE_URL ?? local.url;
const results = [];
await mkdir("artifacts", { recursive: true });
const idle = (page) =>
  page.waitForFunction(() => !document.querySelector("#reset").disabled);
const snapshot = (page) =>
  page.evaluate(
    async () => (await window.coloniesClient.request("snapshot")).result,
  );
async function audit(page, name) {
  const result = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
    .analyze();
  await writeFile(
    `artifacts/accessibility-${name}.json`,
    JSON.stringify(result, null, 2),
  );
  assert.deepEqual(
    result.violations.map((v) => ({
      id: v.id,
      impact: v.impact,
      nodes: v.nodes.map((n) => ({
        target: n.target,
        failureSummary: n.failureSummary,
      })),
    })),
    [],
    `${name} accessibility violations`,
  );
}
async function verifyRollingPlayback(browser) {
  const page = await browser.newPage();
  try {
    // Exercise real timer ticks and expiry quickly, using the engine's smaller
    // test window. Production defaults are stress-tested in verify-retention.
    await page.addInitScript(() => {
      const NativeWorker = window.Worker;
      window.Worker = class extends NativeWorker {
        postMessage(message, ...rest) {
          if (message.type === "init")
            message = {
              ...message,
              args: { ...message.args, historyLimit: 32, segmentWrites: 48 },
            };
          super.postMessage(message, ...rest);
        }
      };
    });
    await page.goto(url);
    await idle(page);
    await page.locator("#speed").evaluate((input) => {
      input.value = 20;
    });
    await page.locator("#play").click();
    await page.waitForFunction(
      () => Number(document.querySelector("#generation").textContent) >= 45,
    );
    assert.equal(await page.locator("#pause").isVisible(), true);
    assert.equal(await page.locator("#error").textContent(), "");
    await page.locator("#pause").click();
    await idle(page);
    assert.ok(
      (await snapshot(page)).branches.every((b) => b.checkpointCount === 32),
    );
    assert.equal(await page.locator("#history").getAttribute("max"), "31");
    assert.match(
      await page.locator("#history-start").textContent(),
      /^Gen [1-9]/,
    );
    await page.locator('[data-branch="experiment-2"]').click();
    await idle(page);
    assert.equal(
      await page.locator("#play").isVisible(),
      true,
      "Paused selection stays paused",
    );
    const history = await page.evaluate(
      async () =>
        (
          await window.coloniesClient.request("history", {
            name: "experiment-2",
          })
        ).result,
    );
    await page.locator("#history").evaluate((input) => {
      input.value = 0;
      input.dispatchEvent(new Event("input"));
    });
    await idle(page);
    assert.equal(
      Number(await page.locator("#generation").textContent()),
      history[0].generation,
    );
    await page.locator("#focus").focus();
    await page.keyboard.press("Enter");
    await idle(page);
    await page.locator("#play").click();
    await page.waitForFunction(
      (generation) =>
        Number(document.querySelector("#generation").textContent) >
        generation + 2,
      history[0].generation,
    );
    await page.locator("#pause").click();
    await idle(page);
    const child = (await snapshot(page)).branches.at(-1);
    assert.equal(child.parent.checkpoint, history[0].id);
    assert.equal(child.parentAvailable, false);
    await page.locator("#details-panel > summary").click();
    assert.equal(await page.locator("#visit-origin").isDisabled(), true);
    assert.equal(
      await page.locator('[data-origin="fork-1"]').isDisabled(),
      true,
    );
    assert.equal(await page.locator("#error").textContent(), "");
  } finally {
    await page.close();
  }
}
try {
  for (const [name, type] of Object.entries({ chromium, firefox, webkit })) {
    const browser = await type.launch({ headless: true });
    try {
      const context = await browser.newContext({
        viewport: { width: 1440, height: 1000 },
        reducedMotion: "reduce",
      });
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (e) => errors.push(e.message));
      await page.goto(url);
      await idle(page);
      assert.equal(await page.locator(".colony-card").count(), 6);
      if (process.env.COLONIES_SITE_URL) {
        assert.equal(
          await page.locator('link[rel="canonical"]').getAttribute("href"),
          "https://stratadb.org/demos/colonies/",
        );
        assert.equal(await page.locator("iframe").count(), 0);
      }
      await audit(page, `${name}-initial`);
      await page.locator("#help").click();
      await page.locator("#help-dialog").waitFor({ state: "visible" });
      await audit(page, `${name}-help`);
      await page.keyboard.press("Escape");
      assert.equal(await page.locator("#help-dialog").isVisible(), false);
      assert.equal(
        await page
          .locator("#help")
          .evaluate((el) => el === document.activeElement),
        true,
      );
      assert.equal(
        await page.locator("#details-panel").getAttribute("open"),
        null,
      );
      assert.equal(await page.locator("#quickstart").count(), 0);
      const visibleButtons = await page
        .locator("button")
        .evaluateAll(
          (buttons) =>
            buttons.filter(
              (b) =>
                !b.closest("details:not([open])") &&
                b.getClientRects().length &&
                getComputedStyle(b).visibility !== "hidden",
            ).length,
        );
      assert.ok(
        visibleButtons <= 10,
        `Too many initial controls: ${visibleButtons}`,
      );
      await page.locator("#play").click();
      await page.waitForFunction(
        () => Number(document.querySelector("#generation").textContent) >= 4,
      );
      for (const name of ["experiment-2", "control", "experiment-1"]) {
        await page.locator(`[data-branch="${name}"]`).click();
        await idle(page);
        assert.equal(await page.locator("#pause").isVisible(), true);
        assert.equal(await page.locator("#state-badge").textContent(), "Live");
        const before = Number(await page.locator("#generation").textContent());
        await page.waitForFunction(
          (generation) =>
            Number(document.querySelector("#generation").textContent) >
            generation,
          before,
        );
      }
      await page.locator("#pause").click();
      await idle(page);
      const parentFuture = (await snapshot(page)).branches[1];
      const source = await page.evaluate(async () => {
        const moments = (
          await window.coloniesClient.request("history", {
            name: "experiment-1",
          })
        ).result;
        const index = Math.floor((moments.length - 1) / 2);
        const input = document.querySelector("#history");
        input.value = index;
        input.dispatchEvent(new Event("input"));
        return moments[index];
      });
      await idle(page);
      assert.equal(
        await page.locator("#state-badge").textContent(),
        "In the past",
      );
      await page.locator("#focus").focus();
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await idle(page);
      assert.equal(await page.locator("#quick-change").isVisible(), true);
      await page.locator("#quick-undo").click();
      assert.equal(
        await page.locator("#change-note").textContent(),
        "0 cells changed",
      );
      await page.locator("#focus").focus();
      await page.keyboard.press("Enter");
      await idle(page);
      await audit(page, `${name}-draft`);
      await page.locator("#play").click();
      await page.waitForFunction(
        () =>
          document.querySelectorAll(".colony-card").length === 7 &&
          !document.querySelector("#pause").hidden,
      );
      await page.waitForFunction(
        (g) => Number(document.querySelector("#generation").textContent) > g,
        source.generation,
      );
      await page.locator("#pause").click();
      await idle(page);
      const child = (await snapshot(page)).branches.at(-1);
      assert.equal(child.parent.checkpoint, source.id);
      const retained = await page.evaluate(
        async (id) =>
          (
            await window.coloniesClient.request("read", {
              name: "experiment-1",
              checkpoint: id,
            })
          ).result,
        parentFuture.head.id,
      );
      assert.equal(retained.board, parentFuture.board);
      assert.equal(
        await page.locator("#details-panel").getAttribute("open"),
        null,
        "The main journey must not need advanced controls",
      );
      await page.locator("#details-panel > summary").click();
      await page.locator("#compare-target").selectOption("parent");
      await page.locator("#view-split").click();
      await page.waitForFunction(
        () => !document.querySelector("#reference").hidden,
      );
      await audit(page, `${name}-split`);
      await page.locator('[data-origin="fork-1"]').click();
      await idle(page);
      assert.equal(await page.locator("#selected").textContent(), "Colony 01");
      const beforeReplay = (await snapshot(page)).branches[1];
      await page.locator("#play").click();
      await page.waitForFunction(
        () => document.querySelector("#state-badge").textContent === "Paused",
      );
      assert.deepEqual((await snapshot(page)).branches[1], beforeReplay);
      await page.reload();
      await idle(page);
      assert.equal(
        await page.locator("#details-panel").getAttribute("open"),
        null,
      );
      assert.equal((await snapshot(page)).branches.length, 6);
      assert.equal((await snapshot(page)).branches[0].head.generation, 0);
      await page.setViewportSize({ width: 390, height: 844 });
      assert.equal(
        await page.evaluate(
          () => document.documentElement.scrollWidth > innerWidth,
        ),
        false,
      );
      assert.equal(await page.locator("#editing-panel").isVisible(), false);
      assert.equal(await page.locator("#fork").isVisible(), true);
      await audit(page, `${name}-mobile`);
      await page.screenshot({
        path: `artifacts/release-${name}-mobile.png`,
        fullPage: true,
      });
      assert.deepEqual(errors, []);
      await verifyRollingPlayback(browser);
      results.push({
        browser: name,
        version: browser.version(),
        passed: true,
        checks: [
          "Switching futures keeps all colonies running and the selected board advancing",
          "Live playback crosses the retention boundary; rewind and exact forks work after expiry",
          "Play, rewind, keyboard nudge, one-button application, exact automatic fork and preserved history",
          "Accessible help and focus return",
          "Overlay and paired view",
          "Advanced controls stay closed throughout the main journey and after reload",
          "No mobile overflow",
          "No automated WCAG A/AA violations in initial, help, draft, split and mobile states",
        ],
      });
      console.log(
        `PASS ${name}: simple main journey, history preservation, accessibility and mobile layout`,
      );
    } finally {
      await browser.close();
    }
  }
  await writeFile(
    "artifacts/release-verification.json",
    JSON.stringify({ url, results }, null, 2) + "\n",
  );
} finally {
  if (local) await new Promise((resolve) => local.server.close(resolve));
}
