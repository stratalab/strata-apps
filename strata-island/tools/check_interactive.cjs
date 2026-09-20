const { chromium } = require(
  process.env.ISLAND_PLAYWRIGHT_PATH || "playwright",
);
const assert = require("node:assert/strict");
const fs = require("node:fs");
const output = process.env.ISLAND_UI_OUTPUT || "/tmp/island-interactive-ui";
fs.mkdirSync(output, { recursive: true });
(async () => {
  const browser = await chromium.launch({
    executablePath: process.env.ISLAND_CHROMIUM,
    headless: true,
    args: ["--no-sandbox"],
  });
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1000 },
    reducedMotion: "reduce",
  });
  page.setDefaultTimeout(120000); // Full graph forks are measured separately; allow completion here.
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const base = process.env.ISLAND_URL || "http://127.0.0.1:7453";
  async function api(path, body) {
    const r = await page.request.fetch(
      `${base}/api/${path}`,
      body
        ? { method: "POST", data: body, timeout: 120000 }
        : { timeout: 120000 },
    );
    assert.equal(r.status(), 200, `${path}: ${await r.text()}`);
    return r.json();
  }
  const meta = await api("meta");
  assert.ok(
    !meta.durable ||
      (process.env.ISLAND_TEST_DB?.startsWith("/tmp/island-") &&
        meta.db_path === process.env.ISLAND_TEST_DB),
    "Requires cache mode or an explicitly named /tmp/island-* test database",
  );
  for (const b of meta.branches)
    if (b.name.startsWith("desk-")) await api("archive", { desk: b.name });
  await page.goto(base);
  const idle = () =>
    page.waitForFunction(
      () =>
        document.body.dataset.ready === "true" &&
        !document.body.classList.contains("busy"),
    );
  await idle();
  await page.locator("#nav-closures").click();
  assert.equal(await page.locator("#closure-picker").inputValue(), "custom");
  assert.equal(await page.locator("#btn-close").isDisabled(), true);
  await page.locator("#closure-search").fill("West 34th Street");
  const section = page.locator(".closure-section").first();
  assert.equal(
    await section.locator("strong").textContent(),
    "West 34th Street",
  );
  const key = await section.getAttribute("data-street-key");
  await section.click();
  assert.match(
    await page.locator("#closure-selection-status").textContent(),
    /^1 street section/,
  );
  await page.locator("#btn-pick-streets").click();
  const box = await page.locator("#plat").boundingBox();
  const panel = await page.locator("#activity-panel").boundingBox();
  await page.mouse.click(
    (panel.x + panel.width + 36 + box.width - 86) / 2,
    (32 + box.height - 48) / 2,
  );
  assert.equal(
    await page.locator("#closure-selection-status").textContent(),
    "No street sections selected.",
  );
  await page.keyboard.press("Escape");
  assert.equal(
    await page.locator("#btn-pick-streets").getAttribute("aria-pressed"),
    "false",
  );
  await page.locator("#closure-undo").click();
  await page.locator("#closure-clear").click();
  assert.equal(await page.locator("#btn-close").isDisabled(), true);
  await page.locator("#closure-undo").click();
  await page.locator("#closure-name").fill("34th Street festival");
  const city = await api("city");
  const [a, b] = key.split(":").map(Number);
  const expected = city.edges.filter(
    (e) => (e.s === a && e.d === b) || (e.s === b && e.d === a),
  );
  await page.screenshot({ path: `${output}/closure-preview.png` });
  await page.locator("#btn-close").click();
  await idle();
  let current = await api("meta");
  let desk = current.branches.find((b) => b.name.startsWith("desk-")).name;
  const history = await api(`scenarios/${desk}/history`);
  assert.equal(history.name, "34th Street festival");
  assert.equal(history.closure.edges.length, expected.length);
  for (const e of expected)
    assert(
      history.closure.edges.some(
        (d) => d.src === city.nodes[e.s].id && d.dst === city.nodes[e.d].id,
      ),
    );
  assert.equal(
    (await api(`city?branch=${desk}`)).edges.length,
    city.edges.length - expected.length,
  );
  assert.equal((await api("city")).edges.length, city.edges.length);
  await page.locator("#nav-closures").click();
  await page.locator("#btn-reopen").click();
  await idle();
  assert.equal(
    (await api(`city?branch=${desk}`)).edges.length,
    city.edges.length,
  );
  await page.locator("#btn-archive").click();
  await idle();
  await page.locator("#nav-route").click();
  await page.locator("#mode-transit").click();
  await idle();
  await page.locator("#nav-missions").click();
  await page.locator("#mission-start").click();
  await idle();
  assert.equal(
    await page.locator("#mode-car").getAttribute("aria-pressed"),
    "true",
  );
  assert.equal(await page.locator("#mission-travel").isDisabled(), false);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.locator("#mission-travel").click();
  await page.waitForFunction(() =>
    document.querySelector("#mission-status").textContent.includes("Traveling"),
  );
  assert.equal(await page.locator("#from-search").isDisabled(), true);
  await idle();
  await page.emulateMedia({ reducedMotion: "reduce" });
  assert.equal(
    await page.locator("#passport-count").textContent(),
    "1 VISITED",
  );
  assert.equal(
    await page.locator("#mission-progress").getAttribute("value"),
    "1",
  );
  assert.equal(
    await page.locator("#mission-travel").isDisabled(),
    true,
    "Cannot double count a stop",
  );
  const state = await page.evaluate(() =>
    JSON.parse(localStorage.getItem("island-passport-v1")),
  );
  const origin = (
    await api(`places/${encodeURIComponent(state.mission.origin)}`)
  ).place;
  // Detail response format is checked below before creating an actual disconnected origin.
  const node = origin.node;
  const outgoing = city.edges
    .filter((e) => city.nodes[e.s].id === node)
    .map((e) => ({ src: node, dst: city.nodes[e.d].id, edge_type: "street" }));
  const blocked = await api("scenarios", {
    name: "Mission route barrier",
    between: [],
    edges: outgoing,
  });
  await page.reload();
  await idle();
  assert.equal(
    await page.locator("#passport-count").textContent(),
    "1 VISITED",
  );
  await page.locator("#nav-missions").click();
  await page.locator("#mission-plan").click();
  await idle();
  assert.match(
    await page.locator("#mission-status").textContent(),
    /unreachable/,
  );
  assert.equal(await page.locator("#mission-travel").isDisabled(), true);
  await page.locator("#nav-closures").click();
  await page.locator("#btn-reopen").click();
  await idle();
  for (let i = 0; i < 2; i++) {
    await page.locator("#nav-missions").click();
    await page.locator("#mission-plan").click();
    await idle();
    assert.equal(await page.locator("#mission-travel").isDisabled(), false);
    await page.locator("#mission-travel").click();
    await idle();
  }
  assert.match(
    await page.locator("#mission-badges").textContent(),
    /Midtown icons/,
  );
  assert.match(
    await page.locator("#mission-status").textContent(),
    /Mission complete/,
  );
  assert.equal(
    await page.locator("#passport-count").textContent(),
    "3 VISITED",
  );
  await page.screenshot({ path: `${output}/mission-complete.png` });
  await page.reload();
  await idle();
  assert.match(
    await page.locator("#mission-badges").textContent(),
    /Midtown icons/,
  );
  assert.match(
    await page.locator("#mission-status").textContent(),
    /Mission complete/,
  );
  await page.locator("#nav-missions").click();
  for (const theme of ["culture", "parks"]) {
    await page.locator("#mission-theme").selectOption(theme);
    await page.locator("#nav-missions").click();
    await page.locator("#mission-start").click();
    await idle();
    for (let i = 0; i < 3; i++) {
      if (i) {
        await page.locator("#nav-missions").click();
        await page.locator("#mission-plan").click();
        await idle();
      }
      assert.equal(await page.locator("#mission-travel").isDisabled(), false);
      await page.locator("#mission-travel").click();
      await idle();
    }
    assert.match(
      await page.locator("#mission-status").textContent(),
      /Mission complete/,
    );
  }
  assert.equal(
    await page.locator("#passport-count").textContent(),
    "9 VISITED",
  );
  assert.equal(await page.locator(".mission-badge").count(), 3);
  await page.reload();
  await idle();
  assert.equal(await page.locator(".mission-badge").count(), 3);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator("#nav-missions").click();
  await page.locator("#mission-panel").scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${output}/missions-mobile.png` });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
  );
  assert.deepEqual(errors, []);
  console.log(
    "Custom map/search closure, undo/clear, directions, isolation, reopen, missions, blocked-route handling, passport persistence and mobile checks passed.",
  );
  await browser.close();
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
