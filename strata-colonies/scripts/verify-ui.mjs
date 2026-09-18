import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright";
import { serve } from "./serve-browser.mjs";

const { server, url } = await serve(0);
const browser = await chromium.launch({ headless: true });
const errors = [],
  checks = [];
await mkdir(new URL("../artifacts/", import.meta.url), { recursive: true });
const ready = async (page) => {
  await page.waitForFunction(
    () =>
      document.querySelector("#status").textContent.startsWith("6 colonies") &&
      !document.querySelector("#step").disabled,
  );
  await page.evaluate(() => document.fonts.ready);
  await page.locator("#details-panel").evaluate((el) => {
    el.open = true;
  });
  await page.waitForFunction(
    () => !document.querySelector("#timeline-kind").closest(".timeline").hidden,
  );
  if (
    (await page.locator("#compare-toggle").getAttribute("aria-pressed")) ===
    "false"
  )
    await page.locator("#compare-toggle").click();
  await page.evaluate(() => scrollTo(0, 0));
};
const snapshot = (page) =>
  page.evaluate(
    async () => (await window.coloniesClient.request("snapshot")).result,
  );
const waitIdle = (page) =>
  page.waitForFunction(() => !document.querySelector("#reset").disabled);
const shot = (page, name) =>
  page.screenshot({
    path: new URL(`../artifacts/ui-${name}.png`, import.meta.url).pathname,
    fullPage: true,
  });

try {
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1000 },
  });
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(url);
  await ready(page);
  assert.equal(await page.locator(".colony-card").count(), 6);
  assert.equal(await page.locator("#selected").textContent(), "Colony 01");
  assert.equal(await page.locator("#difference").textContent(), "1");
  const layout = await page.evaluate(() => {
    const stage = document
      .querySelector(".stage-column")
      .getBoundingClientRect();
    const controls = document
      .querySelector(".control-panel")
      .getBoundingClientRect();
    return {
      stageRight: stage.right,
      controlsLeft: controls.left,
      fonts: [...document.fonts].map((font) => ({
        family: font.family,
        status: font.status,
      })),
      background: getComputedStyle(document.documentElement).backgroundColor,
      overflow: document.documentElement.scrollWidth > innerWidth,
    };
  });
  assert.ok(layout.controlsLeft > layout.stageRight);
  assert.equal(layout.overflow, false);
  assert.equal(layout.background, "rgb(0, 0, 0)");
  assert.ok(
    layout.fonts.some(
      (font) => font.family === "General Sans" && font.status === "loaded",
    ),
  );
  assert.ok(
    layout.fonts.some(
      (font) => font.family === "Commit Mono" && font.status === "loaded",
    ),
  );
  await shot(page, "desktop");
  checks.push(
    "Desktop: shared fonts load, six previews, right-side controls, no horizontal overflow",
  );

  await page.locator("#play").click();
  await page.waitForFunction(
    () => Number(document.querySelector("#generation").textContent) >= 3,
  );
  await page.locator("#pause").click();
  await waitIdle(page);
  const paused = await snapshot(page);
  await page.waitForTimeout(180);
  assert.equal(
    (await snapshot(page)).branches[0].head.id,
    paused.branches[0].head.id,
  );
  assert.equal(await page.locator("#state-badge").textContent(), "Paused");
  await page.locator("#step").click();
  await waitIdle(page);
  assert.equal(
    (await snapshot(page)).branches[0].head.generation,
    paused.branches[0].head.generation + 1,
  );
  await page.locator("#speed").evaluate((input) => {
    input.value = "12";
    input.dispatchEvent(new Event("input"));
    input.dispatchEvent(new Event("change"));
  });
  await waitIdle(page);
  assert.match(await page.locator("#speed-value").textContent(), /^12 /);
  checks.push("Playback, pause, single-step, and speed controls");

  const beforeEdit = await snapshot(page);
  await page.locator("#focus").click({ position: { x: 40, y: 40 } });
  await waitIdle(page);
  assert.equal(
    (await snapshot(page)).branches[1].board,
    beforeEdit.branches[1].board,
  );
  await page.locator("#apply-edits").click();
  await waitIdle(page);
  const afterEdit = await snapshot(page);
  assert.notEqual(afterEdit.branches[1].board, beforeEdit.branches[1].board);
  assert.equal(
    afterEdit.branches[1].head.revision,
    beforeEdit.branches[1].head.revision + 1,
  );
  await page.locator("#focus").focus();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await waitIdle(page);
  await page.locator("#apply-edits").click();
  await waitIdle(page);
  assert.equal(
    (await snapshot(page)).branches[1].head.revision,
    afterEdit.branches[1].head.revision + 1,
  );
  await page.locator("#compare-toggle").click();
  assert.equal(
    await page.locator("#compare-toggle").getAttribute("aria-pressed"),
    "false",
  );
  await page.locator("#compare-toggle").click();
  checks.push("Pointer and keyboard mutations, comparison toggle");

  const parent = (await snapshot(page)).branches[1];
  await page.locator("#history").evaluate((input) => {
    input.value = "1";
    input.dispatchEvent(new Event("change"));
  });
  await waitIdle(page);
  assert.equal(await page.locator("#state-badge").textContent(), "In the past");
  assert.equal(await page.locator("#play").isDisabled(), false);
  assert.equal(await page.locator("#step").isDisabled(), false);
  assert.equal(await page.locator("#latest").isVisible(), true);
  assert.equal((await snapshot(page)).branches[1].board, parent.board);
  await page.locator("#fork").click();
  await waitIdle(page);
  assert.equal(await page.locator(".colony-card").count(), 7);
  assert.equal(await page.locator("#selected").textContent(), "Your world 01");
  assert.equal(Number(await page.locator("#generation").textContent()), 0);
  assert.equal((await snapshot(page)).branches[1].board, parent.board);
  await page.locator("#step").click();
  await waitIdle(page);
  const aligned = await page.evaluate(
    async () =>
      (
        await window.coloniesClient.request("compare", {
          left: "fork-1",
          right: "control",
          generation: 1,
        })
      ).result.divergence,
  );
  await page.waitForFunction(
    (expected) =>
      Number(
        document.querySelector("#difference").textContent.replaceAll(",", ""),
      ) === expected,
    aligned,
  );
  checks.push(
    "History is read-only, historical forks preserve parent, comparisons align differing branch heads",
  );

  await page.locator('[data-branch="control"]').click();
  await waitIdle(page);
  const control = (await snapshot(page)).branches[0];
  await page.locator("#focus").click({ position: { x: 50, y: 50 } });
  await waitIdle(page);
  await page.locator("#apply-edits").click();
  await waitIdle(page);
  assert.equal(await page.locator(".colony-card").count(), 8);
  assert.equal((await snapshot(page)).branches[0].board, control.board);
  for (let i = 0; i < 4; i++) {
    await page.locator("#fork").click();
    await waitIdle(page);
  }
  assert.equal(await page.locator(".colony-card").count(), 12);
  assert.equal(await page.locator("#fork").isDisabled(), true);
  checks.push(
    "Control edits create a child; branch cap is visible and enforced",
  );

  await page.locator("#reset").click();
  await page.getByRole("button", { name: "Keep exploring" }).click();
  assert.equal(await page.locator(".colony-card").count(), 12);
  await page.locator("#reset").click();
  await page.locator("#confirm-reset").click();
  await ready(page);
  assert.equal((await snapshot(page)).branches[0].head.generation, 0);
  assert.equal(await page.locator("#error").textContent(), "");
  checks.push(
    "Reset confirmation cancels safely or starts a clean six-colony session",
  );

  await page.setViewportSize({ width: 1024, height: 768 });
  await page.waitForFunction(() => {
    const canvas = document.querySelector("#focus");
    const data = canvas
      .getContext("2d")
      .getImageData(0, 0, canvas.width, canvas.height).data;
    for (let i = 0; i < data.length; i += 4)
      if (data[i] > 150 && data[i + 1] > 150 && data[i + 2] > 150) return true;
    return false;
  });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
  );
  await shot(page, "compact");

  const mobileContext = await browser.newContext({
    viewport: { width: 390, height: 844 },
    deviceScaleFactor: 2,
    isMobile: true,
    hasTouch: true,
    reducedMotion: "reduce",
  });
  const mobile = await mobileContext.newPage();
  mobile.on("pageerror", (error) => errors.push(error.message));
  await mobile.goto(url);
  await ready(mobile);
  assert.equal(
    await mobile.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
  );
  const position = await mobile.evaluate(() => ({
    transport: document.querySelector(".transport").getBoundingClientRect().top,
    canvas: document.querySelector("#focus").getBoundingClientRect().top,
  }));
  assert.ok(
    position.transport < position.canvas,
    "Mobile playback must precede the grid",
  );
  await shot(mobile, "mobile");
  await mobile.locator("#focus").scrollIntoViewIfNeeded();
  const box = await mobile.locator("#focus").boundingBox();
  await mobile.touchscreen.tap(box.x + box.width / 2, box.y + box.height / 2);
  await waitIdle(mobile);
  assert.equal((await snapshot(mobile)).branches[1].head.revision, 1);
  await mobile.locator("#apply-edits").click();
  await waitIdle(mobile);
  assert.equal((await snapshot(mobile)).branches[1].head.revision, 2);
  await mobile.evaluate(() => scrollBy(0, 150));
  assert.ok(
    (await mobile.locator(".transport").boundingBox()).y >= -1,
    "Playback should stick while scrolling",
  );
  assert.equal(await mobile.locator("#error").textContent(), "");
  checks.push(
    "Mobile: no overflow, sticky playback, touch mutation, reduced motion",
  );
  await mobileContext.close();

  const failure = await browser.newPage();
  failure.on("pageerror", (error) => errors.push(error.message));
  await failure.route("**/*.wasm", (route) => route.abort());
  await failure.goto(url);
  await failure.locator("#retry").waitFor({ state: "visible" });
  assert.equal(await failure.locator("#play").isDisabled(), true);
  assert.ok((await failure.locator("#error").textContent()).length > 0);
  await failure.unroute("**/*.wasm");
  await failure.locator("#retry").click();
  await ready(failure);
  assert.equal(await failure.locator("#error").textContent(), "");
  checks.push("Failed engine load has a visible error and a working retry");
  assert.deepEqual(errors, []);
  await writeFile(
    new URL("../artifacts/ui-verification.json", import.meta.url),
    JSON.stringify(
      { passed: true, browser: browser.version(), checks },
      null,
      2,
    ) + "\n",
  );
  console.log(checks.map((check) => `PASS: ${check}`).join("\n"));
} finally {
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
}
