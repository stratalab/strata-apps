import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright";
import { serve } from "./serve-browser.mjs";
import { EditDraft } from "../web/edit-draft.mjs";
import { emptyBoard, encode } from "../web/life.mjs";

// Sparse pointer events must still paint every crossed cell, revisits must not
// toggle cells twice, and cancelling a gesture must restore the previous draft.
const model = new EditDraft({
  name: "test",
  checkpoint: {},
  board: encode(emptyBoard(9, 11)),
  width: 9,
  height: 11,
});
model.begin([0, 0], "paint");
model.move([8, 0]);
model.move([0, 0]);
model.end();
assert.equal(model.changes.length, 9);
model.begin([4, 0], "erase");
model.move([4, 10]);
model.end();
assert.equal(model.changes.length, 8);
model.undo();
assert.equal(model.changes.length, 9);
model.begin([0, 1], "paint");
model.move([8, 10]);
model.end(true);
assert.equal(model.changes.length, 9);
model.undo();
assert.equal(model.changes.length, 0);
await mkdir("artifacts", { recursive: true });
const checks = [
  "Continuous, idempotent painting; stroke undo and cancellation; padded board dimensions",
];
const errors = [];
const { server, url } = await serve(0);
const browser = await chromium.launch({ headless: true });
const idle = (page) =>
  page.waitForFunction(() => !document.querySelector("#reset").disabled);
const snapshot = (page) =>
  page.evaluate(
    async () => (await window.coloniesClient.request("snapshot")).result,
  );
const history = (page, name = "experiment-1") =>
  page.evaluate(
    async (name) =>
      (await window.coloniesClient.request("history", { name })).result,
    name,
  );
const scrub = async (page, index) => {
  await page.locator("#history").evaluate((input, index) => {
    input.value = index;
    input.dispatchEvent(new Event("input"));
  }, index);
  await idle(page);
};
const count = (page) => page.locator("#draft-badge").textContent();
const point = async (page, x, y) => {
  const r = await page.locator("#focus").boundingBox();
  return {
    x: r.x + ((x + 0.5) * r.width) / 64,
    y: r.y + ((y + 0.5) * r.height) / 48,
  };
};
const clickCell = async (page, x, y) => {
  await page.locator("#focus").scrollIntoViewIfNeeded();
  const p = await point(page, x, y);
  await page.mouse.click(p.x, p.y);
  await idle(page);
};
const stroke = async (page, start, end) => {
  await page.locator("#focus").scrollIntoViewIfNeeded();
  const a = await point(page, ...start),
    b = await point(page, ...end);
  await page.mouse.move(a.x, a.y);
  await page.mouse.down();
  await page.mouse.move(b.x, b.y);
  await page.mouse.up();
  await idle(page);
};
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
  const initial = await snapshot(page);
  await page.locator('[data-tool="paint"]').click();
  await stroke(page, [0, 0], [12, 0]);
  const firstCount = Number((await count(page)).split(" ")[0]);
  assert.ok(firstCount > 1);
  assert.deepEqual(
    await snapshot(page),
    initial,
    "Draft must not persist or advance any branch",
  );
  await stroke(page, [0, 0], [12, 0]);
  assert.equal(
    await count(page),
    `${firstCount} changed`,
    "Painting twice is idempotent",
  );
  await stroke(page, [0, 1], [12, 1]);
  assert.ok(Number((await count(page)).split(" ")[0]) > firstCount);
  await page.locator("#undo-edit").click();
  assert.equal(await count(page), `${firstCount} changed`);
  await page.locator("#preview-before").click();
  assert.equal(
    await page.locator("#state-badge").textContent(),
    "Before edits",
  );
  await clickCell(page, 20, 20);
  assert.equal(await count(page), `${firstCount} changed`);
  await page.locator("#preview-before").click();
  assert.equal(await page.locator("#history").isDisabled(), true);
  assert.equal(
    await page.locator('[data-branch="control"]').isDisabled(),
    true,
  );
  await page.screenshot({
    path: "artifacts/interaction-draft.png",
    fullPage: true,
  });
  await page.locator("#apply-edits").click();
  await idle(page);
  assert.equal(
    await page
      .locator("#focus")
      .evaluate((el) => el === document.activeElement),
    true,
  );
  const applied = await snapshot(page);
  assert.equal(
    applied.branches[1].checkpointCount,
    initial.branches[1].checkpointCount + 1,
  );
  assert.equal(
    applied.branches[1].head.revision,
    initial.branches[1].head.revision + 1,
  );
  assert.equal(applied.branches[1].head.changes.length, firstCount);
  assert.equal(applied.branches[1].head.generation, 0);
  assert.deepEqual(applied.branches[0], initial.branches[0]);
  const recorded = await page.evaluate(
    async (id) =>
      (
        await window.coloniesClient.request("read", {
          name: "experiment-1",
          checkpoint: id,
        })
      ).result,
    initial.branches[1].head.id,
  );
  assert.equal(recorded.board, initial.branches[1].board);
  checks.push(
    "Drafts preserve the database; before/after inspection; one grouped mutation preserves its earlier revision",
  );

  await stroke(page, [20, 0], [30, 0]);
  await page.locator("#clear-edits").click();
  assert.equal(await count(page), "0 changed");
  assert.equal(await page.locator("#apply-edits").isDisabled(), true);
  await page.locator("#cancel-edits").click();
  assert.deepEqual(await snapshot(page), applied);
  await page.locator('[data-branch="control"]').click();
  await idle(page);
  await clickCell(page, 0, 0);
  assert.equal((await snapshot(page)).branches.length, 6);
  await page.locator("#cancel-edits").click();
  assert.deepEqual(await snapshot(page), applied);
  await page.locator('[data-branch="experiment-1"]').click();
  await idle(page);
  await page.locator('[data-tool="auto"]').click();
  await clickCell(page, 20, 20);
  await page.locator("#focus").focus();
  await page.keyboard.press("Control+z");
  assert.equal(await count(page), "0 changed");
  await page.locator("#cancel-edits").click();
  checks.push(
    "Clear, discard, keyboard undo, and cancelled Control drafts leave history and branch count unchanged",
  );

  for (let i = 0; i < 3; i++) {
    await page.locator("#step").click();
    await idle(page);
  }
  for (let i = 0; i < 2; i++) {
    await clickCell(page, 25 + i, 20);
    await page.locator("#apply-edits").click();
    await idle(page);
  }
  for (let i = 0; i < 2; i++) {
    await page.locator("#step").click();
    await idle(page);
  }
  const future = await snapshot(page),
    moments = await history(page);
  const atThree = moments.findIndex(
    (c) => c.generation === 3 && c.revision === 0,
  );
  await scrub(page, atThree);
  await page.locator("#step").click();
  await idle(page);
  assert.match(
    await page.locator("#checkpoint").textContent(),
    /Generation 3 · revision 1/,
  );
  await page.locator("#step").click();
  await idle(page);
  assert.match(
    await page.locator("#checkpoint").textContent(),
    /Generation 3 · revision 2/,
  );
  assert.deepEqual(await snapshot(page), future);
  await scrub(page, 0);
  await page.locator("#speed").fill("20");
  await page.locator("#play").click();
  await page.waitForFunction(
    () => document.querySelector("#state-badge").textContent === "Paused",
  );
  assert.deepEqual(
    await snapshot(page),
    future,
    "Replay must stop at head without a write",
  );
  await scrub(page, 0);
  await page.locator("#play").click();
  await page.locator("#pause").click();
  await idle(page);
  const pausedMoment = await page.locator("#checkpoint").textContent();
  await page.waitForTimeout(150);
  assert.equal(await page.locator("#checkpoint").textContent(), pausedMoment);
  checks.push(
    "History steps include same-generation revisions; replay and pause never write or simulate beyond head",
  );

  // Coalesce scrubs and invalidate reads that finish after leaving replay.
  await page.locator("#history").evaluate(
    (input, indexes) => {
      for (const i of indexes) {
        input.value = i;
        input.dispatchEvent(new Event("input"));
      }
    },
    [0, 2, atThree],
  );
  await idle(page);
  assert.match(
    await page.locator("#checkpoint").textContent(),
    /Generation 3 · revision 0/,
  );
  await page.evaluate(() => {
    const client = window.coloniesClient,
      original = client.request.bind(client);
    window.restoreRequest = () => {
      client.request = original;
    };
    client.request = async (type, args) => {
      const result = await original(type, args);
      if (type === "read" && args.name === "experiment-1")
        await new Promise((r) => setTimeout(r, 150));
      return result;
    };
  });
  await page.locator("#play").click();
  await page.waitForTimeout(70);
  await page.locator("#latest").click();
  await page.waitForTimeout(200);
  assert.equal(await page.locator("#state-badge").textContent(), "Paused");
  assert.match(
    await page.locator("#checkpoint").textContent(),
    /Generation 5 · revision 0/,
  );
  await page.evaluate(() => window.restoreRequest());
  checks.push(
    "Rapid scrubbing keeps the final selection; stale replay reads cannot restore an exited historical view",
  );

  await scrub(page, atThree);
  await page.locator('[data-tool="paint"]').click();
  await stroke(page, [0, 0], [10, 0]);
  assert.equal((await snapshot(page)).branches.length, 6);
  await page.locator("#apply-edits").click();
  await idle(page);
  const forked = await snapshot(page),
    child = forked.branches.at(-1);
  assert.equal(child.parent.checkpoint, moments[atThree].id);
  assert.equal(child.head.generation, 3);
  assert.equal(child.head.revision, 1);
  assert.equal(child.checkpointCount, 2);
  assert.deepEqual(forked.branches.slice(0, 6), future.branches);
  await page.screenshot({
    path: "artifacts/interaction-fork.png",
    fullPage: true,
  });
  await page.locator('[data-tool="auto"]').click();
  await clickCell(page, 30, 30);
  await page.locator("#apply-run").click();
  await page.waitForFunction(
    () => Number(document.querySelector("#generation").textContent) >= 5,
  );
  await page.locator("#pause").click();
  await idle(page);
  assert.ok((await snapshot(page)).branches.at(-1).head.generation >= 5);
  checks.push(
    "Historical drafts fork the exact revision on apply, preserve parent futures, and run after applying",
  );

  // A mutation begun while a tick is in flight must retain the displayed moment.
  await page.locator("#play").click();
  await clickCell(page, 31, 31);
  assert.equal(
    await page.locator("#state-badge").textContent(),
    "Changing cells",
  );
  const editPaused = await snapshot(page);
  await page.waitForTimeout(150);
  assert.deepEqual(await snapshot(page), editPaused);
  await page.locator("#cancel-edits").click();
  checks.push(
    "Beginning a live edit pauses simulation and holds the captured draft steady",
  );

  const raceBefore = await snapshot(page);
  const raceSource = raceBefore.branches.at(-1);
  await page.evaluate(() => {
    const client = window.coloniesClient,
      original = client.request.bind(client);
    let injected = false;
    client.request = async (type, args) => {
      if (type === "pause" && !injected) {
        injected = true;
        await original("step");
      }
      return original(type, args);
    };
    window.restoreRequest = () => {
      client.request = original;
    };
  });
  await clickCell(page, 32, 32);
  const raced = await snapshot(page);
  assert.equal(
    raced.branches.at(-1).head.generation,
    raceSource.head.generation + 1,
  );
  assert.match(
    await page.locator("#draft-context").textContent(),
    /Applying creates a new branch/,
  );
  await page.locator("#apply-edits").click();
  await idle(page);
  await page.evaluate(() => window.restoreRequest());
  const raceResult = await snapshot(page);
  assert.equal(raceResult.branches.length, raceBefore.branches.length + 1);
  assert.equal(
    raceResult.branches.at(-1).parent.checkpoint,
    raceSource.head.id,
  );
  assert.deepEqual(raceResult.branches.slice(0, -1), raced.branches);
  checks.push(
    "An in-flight tick causes an exact displayed-checkpoint fork instead of overwriting the newer head",
  );

  const mobile = await browser.newPage({
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
  });
  mobile.on("pageerror", (e) => errors.push(e.message));
  await mobile.goto(url);
  await idle(mobile);
  await mobile.locator("#details-panel").evaluate((el) => {
    el.open = true;
  });
  await mobile.locator('[data-tool="paint"]').click();
  await mobile.evaluate(() =>
    document.querySelector(".stage-frame").scrollIntoView({ block: "start" }),
  );
  const a = await point(mobile, 5, 20),
    b = await point(mobile, 25, 20);
  const cdp = await mobile.context().newCDPSession(mobile);
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [{ ...a, id: 1 }],
  });
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchMove",
    touchPoints: [{ ...b, id: 1 }],
  });
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchEnd",
    touchPoints: [],
  });
  await idle(mobile);
  assert.ok(Number((await count(mobile)).split(" ")[0]) > 1);
  const mobileInitial = await snapshot(mobile);
  await mobile.locator("#apply-edits").click();
  await idle(mobile);
  assert.equal(
    (await snapshot(mobile)).branches[1].checkpointCount,
    mobileInitial.branches[1].checkpointCount + 1,
  );
  assert.equal(
    await mobile.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
  );
  await mobile.evaluate(() => scrollTo(0, 0));
  await mobile.screenshot({
    path: "artifacts/interaction-mobile.png",
    fullPage: true,
  });
  checks.push(
    "Touch dragging paints continuously and applies a group without mobile overflow",
  );
  assert.deepEqual(errors, []);
  await mkdir("artifacts", { recursive: true });
  await writeFile(
    "artifacts/interaction-verification.json",
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
