import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright";
import { serve } from "./serve-browser.mjs";
const { server, url } = await serve(0);
const browser = await chromium.launch({ headless: true });
await mkdir("artifacts", { recursive: true });
const checks = [],
  errors = [];
const rpc = (page, type, args) =>
  page.evaluate(
    async ({ type, args }) =>
      (await window.coloniesClient.request(type, args)).result,
    { type, args },
  );
const snapshot = (page) => rpc(page, "snapshot");
const idle = (page) =>
  page.waitForFunction(() => !document.querySelector("#reset").disabled);
const select = async (page, name) => {
  await page.locator(`[data-lineage="${name}"]`).click();
  await idle(page);
};
const step = async (page, count = 1) => {
  for (let i = 0; i < count; i++) {
    await page.locator("#step").click();
    await idle(page);
  }
};
const mutate = async (page) => {
  await page.locator("#focus").click({ position: { x: 25, y: 25 } });
  await idle(page);
  await page.locator("#apply-edits").click();
  await idle(page);
};
const scrub = async (page, index) => {
  await page.locator("#history").evaluate((input, index) => {
    input.value = index;
    input.dispatchEvent(new Event("input"));
  }, index);
  await idle(page);
};
const chartReady = (page) =>
  page.waitForFunction(() =>
    /matching generation/.test(
      document.querySelector("#chart-status").textContent,
    ),
  );
const equalCount = async (page, expected) =>
  page.waitForFunction(
    (expected) =>
      Number(
        document.querySelector("#difference").textContent.replaceAll(",", ""),
      ) === expected,
    expected,
  );
try {
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1000 },
  });
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(url);
  await idle(page);
  await page.locator("#details-panel").evaluate((el) => {
    el.open = true;
  });
  await chartReady(page);
  assert.equal(await page.locator("[data-lineage]").count(), 6);
  await page.locator("#compare-target").selectOption("parent");
  assert.match(
    await page.locator("#comparison-context").textContent(),
    /Original · generation 0, revision 0/,
  );
  await equalCount(page, 1);
  await step(page, 4);
  await mutate(page);
  const source = (await snapshot(page)).branches[1]; // gen 4 revision 1
  await mutate(page); // another same-generation revision, then a saved future
  await step(page, 3);
  const future = await snapshot(page);
  const moments = await rpc(page, "history", { name: "experiment-1" });
  await scrub(
    page,
    moments.findIndex((c) => c.id === source.head.id),
  );
  await page.locator("#fork").click();
  await idle(page);
  const first = (await snapshot(page)).branches.at(-1);
  assert.equal(first.parent.checkpoint, source.head.id);
  await mutate(page);
  const expected = await rpc(page, "compare", {
    left: first.name,
    right: "experiment-1",
    generation: 4,
  });
  await equalCount(page, expected.divergence);
  assert.match(
    await page.locator("#comparison-context").textContent(),
    /Colony 01 · generation 4, revision 2/,
  );
  assert.deepEqual(
    (await snapshot(page)).branches.slice(0, 6),
    future.branches,
  );
  await page.locator("#view-split").click();
  assert.match(
    await page.locator("#selected-board-label").textContent(),
    /Your world 01 · gen 4 · rev 1/,
  );
  assert.match(
    await page.locator("#reference-board-label").textContent(),
    /Colony 01 · gen 4 · rev 2/,
  );
  const boxes = await page.evaluate(() => ({
    a: document.querySelector("#focus").getBoundingClientRect().right,
    b: document.querySelector("#reference").getBoundingClientRect().left,
    referenceCells: [
      ...document
        .querySelector("#reference")
        .getContext("2d")
        .getImageData(0, 0, 768, 576).data,
    ].some((v, i) => i % 4 === 0 && v > 150),
  }));
  assert.ok(boxes.b > boxes.a);
  assert.equal(boxes.referenceCells, true);
  assert.equal(await page.locator("#compare-toggle").isDisabled(), true);
  await page.locator("#reference").click();
  assert.equal(
    await page.locator("#draft-actions").isVisible(),
    false,
    "Reference grid must be read-only",
  );
  await chartReady(page);
  await page.evaluate(() => scrollTo(0, 0));
  await page.screenshot({
    path: "artifacts/branching-desktop.png",
    fullPage: true,
  });
  checks.push(
    "Parent comparison uses the latest revision at the matching generation; side-by-side reference is labeled and read-only",
  );

  const beforeVisit = await snapshot(page);
  await page.locator('[data-origin="fork-1"]').click();
  await idle(page);
  assert.equal(await page.locator("#selected").textContent(), "Colony 01");
  assert.match(
    await page.locator("#checkpoint").textContent(),
    /Generation 4 · revision 1/,
  );
  assert.deepEqual(await snapshot(page), beforeVisit);
  assert.equal(await page.locator('[data-forks="fork-1"]').count(), 1);
  await select(page, "fork-1");
  await page.locator("#fork").click();
  await idle(page);
  await mutate(page);
  const nested = (await snapshot(page)).branches.at(-1);
  assert.equal(nested.parent.name, "fork-1");
  const nestedParent = (await snapshot(page)).branches.find(
    (b) => b.name === "fork-1",
  );
  assert.equal(nested.parent.checkpoint, nestedParent.head.id);
  await equalCount(page, 1);
  assert.equal(
    await page
      .locator('[data-lineage="fork-1"]')
      .locator("xpath=../..")
      .locator('[data-lineage="fork-2"]')
      .count(),
    1,
  );
  assert.match(
    await page.locator("#lineage-context").textContent(),
    /Original → Colony 01 → Your world 01 → Your world 02/,
  );
  await page.locator("#visit-origin").click();
  await idle(page);
  assert.equal(await page.locator("#selected").textContent(), "Your world 01");
  assert.equal(
    await page.locator('[data-lineage="fork-1"]').getAttribute("aria-pressed"),
    "true",
  );
  checks.push(
    "Nested lineage preserves parentage; both origin links visit the exact stored fork revision without writes",
  );

  await select(page, "fork-2");
  const beforeChart = await snapshot(page);
  const series = await rpc(page, "comparison-history", {
    left: "fork-2",
    right: "fork-1",
  });
  assert.deepEqual(series.points, [
    { generation: 4, divergence: 1, leftRevision: 1, rightRevision: 1 },
  ]);
  await mutate(page);
  const changedSeries = await rpc(page, "comparison-history", {
    left: "fork-2",
    right: "fork-1",
  });
  assert.equal(changedSeries.points[0].leftRevision, 2);
  assert.equal(changedSeries.points[0].divergence, 0);
  const oldSeries = await rpc(page, "comparison-history", {
    left: "fork-2",
    right: "fork-1",
    leftHead: beforeChart.branches.at(-1).head.id,
    rightHead: nestedParent.head.id,
  });
  assert.deepEqual(
    oldSeries,
    series,
    "Chart requests respect captured heads even after a newer revision",
  );
  await chartReady(page);
  await page.locator("#chart-values summary").click();
  await page.waitForFunction(() =>
    document.querySelector("#chart-rows")?.textContent.endsWith("0"),
  );
  assert.match(
    await page.locator("#divergence-chart").getAttribute("aria-label"),
    /generations 4 to 4/,
  );
  assert.equal(await page.locator("#chart-rows tr").count(), 1);
  checks.push(
    "Chart uses one latest revision per shared generation, invalidates edited points, and exposes exact values in a table",
  );

  // Advance only the child to produce a missing reference generation.
  await rpc(page, "step", { names: ["fork-2"] });
  await select(page, "fork-2");
  assert.equal(await page.locator("#difference").textContent(), "—");
  assert.match(
    await page.locator("#comparison-context").textContent(),
    /Your world 01 has no saved board at generation 5/,
  );
  assert.equal(await page.locator("#reference").isVisible(), false);
  assert.equal(await page.locator("#reference-unavailable").isVisible(), true);
  await page.locator("#compare-target").selectOption("control");
  const versusControl = await rpc(page, "compare", {
    left: "fork-2",
    right: "control",
    generation: 5,
  });
  await equalCount(page, versusControl.divergence);
  await select(page, "control");
  await page.locator("#compare-target").selectOption("parent");
  assert.match(
    await page.locator("#comparison-context").textContent(),
    /Original has no parent/,
  );
  checks.push(
    "Missing generations and absent parents have explicit empty states; switching reference restores an aligned comparison",
  );

  await select(page, "fork-2");
  await page.locator("#compare-target").selectOption("control");
  await page.locator("#focus").click({ position: { x: 40, y: 40 } });
  await idle(page);
  assert.equal(await page.locator('[data-origin="fork-2"]').isDisabled(), true);
  assert.equal(
    await page.locator('[data-lineage="control"]').isDisabled(),
    true,
  );
  await page.locator("#cancel-edits").click();
  checks.push("Lineage navigation cannot discard a pending draft");

  // Delay an uncached parent read and change the target while it is pending.
  await rpc(page, "step", { names: ["fork-1"] });
  await page.evaluate(() => {
    const client = window.coloniesClient,
      original = client.request.bind(client);
    window.delayedReferenceReads = 0;
    window.restoreRequest = () => {
      client.request = original;
    };
    client.request = async (type, args) => {
      const result = await original(type, args);
      if (type === "read" && args.name === "experiment-1") {
        window.delayedReferenceReads++;
        await new Promise((r) => setTimeout(r, 150));
      }
      return result;
    };
  });
  await select(page, "fork-1");
  await page.locator("#compare-target").selectOption("parent");
  await page.waitForFunction(() => window.delayedReferenceReads > 0);
  await page.locator("#compare-target").selectOption("control");
  await page.waitForTimeout(200);
  assert.match(
    await page.locator("#comparison-context").textContent(),
    /^Original/,
  );
  await page.evaluate(() => window.restoreRequest());
  checks.push(
    "Late reference reads do not replace the currently selected comparison",
  );

  // New, uncached old generation; injected failures must not trigger a read loop.
  await select(page, "experiment-3");
  await page.evaluate(() => {
    const client = window.coloniesClient,
      original = client.request.bind(client);
    window.failedReads = 0;
    window.restoreRequest = () => {
      client.request = original;
    };
    client.request = async (type, args) => {
      if (type === "read" && args.name === "control") {
        window.failedReads++;
        throw new Error("Temporary read failure");
      }
      return original(type, args);
    };
  });
  await scrub(page, 7); // generation 6 is not previously used as an overlay reference
  await page.locator("#comparison-retry").waitFor({ state: "visible" });
  await page.waitForTimeout(80);
  assert.equal(await page.evaluate(() => window.failedReads), 1);
  await page.evaluate(() => window.restoreRequest());
  await page.locator("#comparison-retry").click();
  await page.waitForFunction(() =>
    document
      .querySelector("#comparison-context")
      .textContent.startsWith("Original · generation 6"),
  );
  checks.push(
    "Failed reference reads display retry and recover without a repeated request loop",
  );

  await page.evaluate(() => {
    const client = window.coloniesClient,
      original = client.request.bind(client);
    window.restoreRequest = () => {
      client.request = original;
    };
    client.request = async (type, args) => {
      if (type === "comparison-history")
        throw new Error("Temporary chart failure");
      return original(type, args);
    };
  });
  await select(page, "experiment-4");
  await page.locator("#chart-retry").waitFor({ state: "visible" });
  assert.equal(await page.locator("#divergence-chart").isVisible(), false);
  await page.evaluate(() => window.restoreRequest());
  await page.locator("#chart-retry").click();
  await chartReady(page);
  checks.push(
    "A failed chart clears stale values and retries independently of the simulation",
  );

  // Reach the branch cap through nested UI forks, then check the narrow layout.
  if (await page.locator("#latest").isVisible())
    await page.locator("#latest").click();
  while ((await snapshot(page)).branches.length < 12) {
    await page.locator("#fork").click();
    await idle(page);
  }
  assert.equal(await page.locator("[data-lineage]").count(), 12);
  assert.equal(await page.locator("#fork").isDisabled(), true);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator("#view-split").click();
  await page.waitForTimeout(50);
  const mobile = await page.evaluate(() => ({
    overflow: document.documentElement.scrollWidth > innerWidth,
    editBottom: document.querySelector(".edit-section").getBoundingClientRect()
      .bottom,
    comparisonBottom: document
      .querySelector(".branch-details")
      .getBoundingClientRect().bottom,
    lineageTop: document
      .querySelector(".branch-explorer")
      .getBoundingClientRect().top,
    first: document.querySelector("#focus").getBoundingClientRect().bottom,
    second: document.querySelector("#reference").getBoundingClientRect().top,
  }));
  assert.equal(mobile.overflow, false);
  assert.ok(
    mobile.editBottom < mobile.lineageTop &&
      mobile.comparisonBottom < mobile.lineageTop,
    "Mobile controls must precede the growing lineage and chart",
  );
  assert.ok(mobile.second > mobile.first);
  await page.evaluate(() => scrollTo(0, 0));
  await page.screenshot({
    path: "artifacts/branching-mobile.png",
    fullPage: true,
  });
  await page.locator("#reset").click();
  await page.locator("#confirm-reset").click();
  await page.waitForFunction(
    () =>
      document.querySelectorAll("[data-lineage]").length === 6 &&
      !document.querySelector("#reset").disabled,
  );
  assert.equal(await page.locator("[data-lineage]").count(), 6);
  assert.equal(await page.locator("#compare-target").inputValue(), "control");
  assert.equal(await page.locator("#reference-board").isVisible(), false);
  await chartReady(page);
  assert.deepEqual(
    (
      await rpc(page, "comparison-history", {
        left: "experiment-1",
        right: "control",
      })
    ).points,
    [{ generation: 0, divergence: 1, leftRevision: 1, rightRevision: 0 }],
  );
  checks.push(
    "Twelve branches fit the narrow layout; paired boards stack; reset clears lineage and comparison caches",
  );
  const deep = await browser.newPage();
  deep.on("pageerror", (e) => errors.push(e.message));
  await deep.goto(url);
  await idle(deep);
  await deep.locator("#details-panel").evaluate((el) => {
    el.open = true;
  });
  await chartReady(deep);
  await rpc(deep, "benchmark", { steps: 1000 });
  const deepBefore = await snapshot(deep);
  const timed = await deep.evaluate(async () => {
    const start = performance.now();
    const result = (
      await window.coloniesClient.request("comparison-history", {
        left: "experiment-1",
        right: "control",
      })
    ).result;
    return { result, elapsedMs: performance.now() - start };
  });
  assert.equal(timed.result.points.length, 1001);
  const deepCompare = await rpc(deep, "compare", {
    left: "experiment-1",
    right: "control",
    generation: 1000,
  });
  assert.equal(timed.result.points.at(-1).divergence, deepCompare.divergence);
  assert.deepEqual(await snapshot(deep), deepBefore);
  checks.push(
    `Chart reads all 1,001 recorded generations without writes (${timed.elapsedMs.toFixed(1)} ms on this machine)`,
  );
  assert.deepEqual(errors, []);
  await writeFile(
    "artifacts/branching-verification.json",
    JSON.stringify(
      { passed: true, browser: browser.version(), checks },
      null,
      2,
    ) + "\n",
  );
  console.log(checks.map((c) => `PASS: ${c}`).join("\n"));
} finally {
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
}
