import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir, cpus, platform, arch } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { chromium } from "playwright";
import { serve } from "./serve-browser.mjs";
import { genesis, perturbation, flip, step, encode } from "../web/life.mjs";

const report = {
  date: new Date().toISOString(),
  platform: `${platform()} ${arch()}`,
  cpu: cpus()[0]?.model,
  checks: [],
  benchmarks: [],
};
const artifact = JSON.parse(
  await readFile(new URL("../web/pkg/manifest.json", import.meta.url)),
);
for (const [name, expected] of Object.entries(artifact.files)) {
  const data = await readFile(new URL(`../web/pkg/${name}`, import.meta.url));
  assert.equal(
    createHash("sha256").update(data).digest("hex"),
    expected.sha256,
    `Artifact changed: ${name}`,
  );
}
report.artifact = artifact;
const temp = await mkdtemp(join(tmpdir(), "colonies-parity-"));
let reference;
try {
  execFileSync("rustc", [
    "--edition=2021",
    "-O",
    "scripts/native-fixture.rs",
    "-o",
    join(temp, "fixture"),
  ]);
  reference = execFileSync(join(temp, "fixture"), { encoding: "utf8" })
    .trim()
    .split("\n")
    .map((line) => {
      const [width, height, seed, index, generation, hex] = line.split(",");
      return {
        width: +width,
        height: +height,
        seed: +seed,
        index: +index,
        generation: +generation,
        board: Buffer.from(hex, "hex").toString("base64"),
      };
    });
} finally {
  await rm(temp, { recursive: true, force: true });
}

for (const [width, height, seed] of [
  [64, 48, 42],
  [16, 12, 7],
  [9, 11, 19],
]) {
  const initial = genesis(width, height, seed),
    used = new Set();
  let boards = [initial];
  for (let i = 1; i <= 5; i++) {
    const cell = perturbation(initial, width, height, i, used);
    used.add(cell.join(","));
    const board = initial.slice();
    flip(board, width, ...cell);
    boards.push(board);
  }
  for (let generation = 0; generation <= 20; generation++) {
    for (let index = 0; index < boards.length; index++) {
      assert.equal(
        encode(boards[index]),
        reference.find(
          (r) =>
            r.width === width &&
            r.seed === seed &&
            r.index === index &&
            r.generation === generation,
        ).board,
      );
    }
    boards = boards.map((board) => step(board, width, height));
  }
}
report.checks.push(
  `Rust/browser parity: ${reference.length} boards, three grid sizes including non-byte-aligned cells`,
);

const { server, url } = await serve(0);
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
const errors = [];
page.on("pageerror", (error) => errors.push(error.message));
try {
  await page.goto(url);
  await page.waitForFunction(
    () =>
      document.querySelector("#status").textContent.startsWith("6 colonies") ||
      document.querySelector("#error").textContent,
  );
  assert.equal(await page.locator("#error").textContent(), "");
  report.browser = browser.version();
  const correctness = await page.evaluate(
    async (expected) => {
      const client = window.coloniesClient;
      const request = async (type, args) =>
        (await client.request(type, args)).result;
      const check = (condition, message) => {
        if (!condition) throw new Error(message);
      };
      const rejects = async (type, args) => {
        try {
          await request(type, args);
        } catch {
          return;
        }
        throw new Error(`Expected ${type} to reject`);
      };
      const initial = await request("snapshot");
      const original = initial.branches[1];
      for (let i = 0; i < 20; i++) await request("step");
      const evolved = await request("snapshot");
      evolved.branches.forEach((branch, i) =>
        check(
          branch.board ===
            expected.find((r) => r.index === i && r.generation === 20).board,
          "Persisted evolution differs from Rust",
        ),
      );
      const history = await request("read", {
        name: original.name,
        checkpoint: original.head.id,
      });
      check(
        history.board === original.board,
        "History changed after evolution",
      );
      const parentFuture = evolved.branches[1].board;
      const edit = await request("mutate", {
        name: original.name,
        cells: [
          [1, 1],
          [2, 1],
        ],
      });
      const edited = edit.branches[1];
      check(
        edited.head.generation === 20 && edited.head.revision === 1,
        "Revision must be separate from generation",
      );
      check(
        (
          await request("read", {
            name: original.name,
            checkpoint: evolved.branches[1].head.id,
          })
        ).board === parentFuture,
        "Edit overwrote prior revision",
      );
      await request("fork", {
        source: original.name,
        checkpoint: original.head.id,
        name: "past-fork",
      });
      const forked = (await request("snapshot")).branches.at(-1);
      check(
        forked.board === original.board && forked.head.generation === 0,
        "Fork did not restore past board",
      );
      await request("mutate", { name: "past-fork", cells: [[3, 3]] });
      const changed = (await request("snapshot")).branches.at(-1);
      check(
        changed.board !== forked.board,
        "Child mutation did not change board",
      );
      check(
        (await request("snapshot")).branches[1].board === edited.board,
        "Child changed parent future",
      );
      await request("fork", {
        source: "past-fork",
        checkpoint: changed.head.id,
        name: "nested-fork",
      });
      const nested = (await request("snapshot")).branches.at(-1);
      check(
        nested.board === changed.board && nested.parent.name === "past-fork",
        "Nested fork lost lineage",
      );
      await request("step", { names: ["past-fork"] });
      check(
        (await request("snapshot")).branches.find(
          (b) => b.name === "nested-fork",
        ).board === nested.board,
        "Parent step changed existing child",
      );
      const comparison = await request("compare", {
        left: original.name,
        right: "past-fork",
        generation: 0,
      });
      check(
        comparison.divergence === 1,
        "Same-generation comparison is incorrect",
      );
      await rejects("compare", {
        left: "past-fork",
        right: original.name,
        generation: 20,
      });
      await rejects("read", {
        name: original.name,
        checkpoint: "does-not-exist",
      });
      await rejects("mutate", { name: "control", cells: [[0, 0]] });
      await rejects("mutate", { name: original.name, cells: [[64, 0]] });
      await rejects("fork", { source: "control", name: "control" });
      const { ColoniesClient } = await import("./client.mjs");
      let liveTicks = 0;
      const isolated = new ColoniesClient((event) => {
        if (event.event === "tick") liveTicks++;
      });
      const other = (await isolated.request("init")).result;
      check(
        other.branches.length === 6 &&
          other.branches[1].board === original.board,
        "Visitor sessions are not isolated",
      );
      await isolated.request("run", { hz: 20 });
      const deadline = performance.now() + 3000;
      while (liveTicks < 2 && performance.now() < deadline)
        await new Promise((resolve) => setTimeout(resolve, 20));
      check(liveTicks >= 2, "Scheduled playback did not emit updates");
      const paused = (await isolated.request("pause")).result;
      await new Promise((resolve) => setTimeout(resolve, 150));
      check(
        (await isolated.request("snapshot")).result.branches[0].head.id ===
          paused.branches[0].head.id,
        "Pause allowed another scheduled tick",
      );
      const pendingInit = isolated.request("init");
      const pendingStep = isolated.request("step");
      await pendingInit;
      check(
        (await pendingStep).result.branches[0].head.generation === 1,
        "Worker requests raced initialization",
      );
      await isolated.request("dispose");
      try {
        await isolated.request("snapshot");
        throw new Error("disposed session accepted a read");
      } catch (error) {
        check(
          error.message.includes("Initialize"),
          "Unexpected dispose result",
        );
      }
      isolated.terminate();
      return {
        checks: [
          "real historical reads after evolution and edits",
          "same-generation revisions",
          "retained-version fork and nested lineage",
          "parent/child write isolation",
          "generation-aligned comparisons",
          "invalid inputs and unavailable history reject",
          "isolated sessions and worker disposal",
          "scheduled playback, pause, and serialized initialization",
        ],
      };
    },
    reference.filter((r) => r.width === 64),
  );
  report.checks.push(...correctness.checks);
  console.log("Correctness checks passed. Measuring six and twelve colonies…");

  for (const branchCount of [6, 12]) {
    const measurement = await page.evaluate(async (branchCount) => {
      window.coloniesClient.terminate();
      const { ColoniesClient } = await import("./client.mjs");
      const { decode, live } = await import("./life.mjs");
      const canvases = Array.from({ length: branchCount }, () => {
        const canvas = document.createElement("canvas");
        canvas.width = 128;
        canvas.height = 96;
        return canvas;
      });
      document.querySelector("#colonies").replaceChildren(...canvases);
      let frames = 0,
        last = performance.now(),
        largestFrameGap = 0,
        active = false;
      const gaps = [],
        longTasks = [];
      const observer = new PerformanceObserver((list) => {
        if (active) longTasks.push(...list.getEntries().map((e) => e.duration));
      });
      observer.observe({ type: "longtask" });
      let animation;
      const frame = (now) => {
        if (active) {
          gaps.push(now - last);
          largestFrameGap = Math.max(largestFrameGap, now - last);
          frames++;
        }
        last = now;
        animation = requestAnimationFrame(frame);
      };
      animation = requestAnimationFrame(frame);
      const client = new ColoniesClient((event) => {
        if (event.event !== "benchmark-tick") return;
        event.result.branches.forEach((branch, i) => {
          const canvas = canvases[i],
            ctx = canvas.getContext("2d"),
            board = decode(branch.board);
          ctx.fillStyle = "#080808";
          ctx.fillRect(0, 0, 128, 96);
          ctx.fillStyle = "#ffffff";
          for (let y = 0; y < 48; y++)
            for (let x = 0; x < 64; x++) {
              if (live(board, 64, x, y)) ctx.fillRect(x * 2, y * 2, 2, 2);
            }
        });
      });
      const initialized = (await client.request("init")).result;
      for (let i = 6; i < branchCount; i++)
        await client.request("fork", {
          source: "experiment-1",
          name: `extra-${i}`,
        });
      const snapshot = (await client.request("snapshot")).result;
      const checkpoints = snapshot.branches.map((b) => ({
        name: b.name,
        checkpoint: b.head.id,
        board: b.board,
      }));
      await new Promise((resolve) => requestAnimationFrame(resolve));
      active = true;
      const started = performance.now();
      const result = (await client.request("benchmark", { steps: 1000 }))
        .result;
      const wallMs = performance.now() - started;
      await new Promise((resolve) => requestAnimationFrame(resolve));
      active = false;
      const historicalReadMs = [];
      for (const checkpoint of checkpoints) {
        const read = await client.request("read", checkpoint);
        if (read.result.board !== checkpoint.board)
          throw new Error(
            "History at generation zero was lost by generation 1000",
          );
        historicalReadMs.push(read.elapsedMs);
      }
      const middle = (
        await client.request("history", { name: "experiment-1" })
      ).result.find((c) => c.generation === 500);
      const middleRead = await client.request("read", {
        name: "experiment-1",
        checkpoint: middle.id,
      });
      if (middleRead.result.checkpoint.generation !== 500)
        throw new Error("Mid-history lookup failed");
      let historicalForkMs = null;
      if (branchCount === 6) {
        const forked = await client.request("fork", {
          source: "experiment-1",
          checkpoint: checkpoints[1].checkpoint,
          name: "deep-fork",
        });
        historicalForkMs = forked.elapsedMs;
        if (forked.result.branches.at(-1).board !== checkpoints[1].board)
          throw new Error("Deep historical fork lost the board");
      } else {
        try {
          await client.request("fork", { source: "control", name: "over-cap" });
          throw new Error("Branch cap failed");
        } catch (error) {
          if (!error.message.includes("12-colony")) throw error;
        }
      }
      const continued = (await client.request("step", { names: ["control"] }))
        .result;
      if (continued.branches[0].head.generation !== 1001)
        throw new Error(
          "Playback did not continue beyond the old generation cap",
        );
      await client.request("dispose");
      client.terminate();
      observer.disconnect();
      cancelAnimationFrame(animation);
      gaps.sort((a, b) => a - b);
      return {
        ...result,
        engineVersion: initialized.engineVersion,
        wallMs,
        frames,
        largestFrameGapMs: largestFrameGap,
        p95FrameGapMs: gaps[Math.ceil(gaps.length * 0.95) - 1],
        mainThreadLongTasks: longTasks.length,
        maxLongTaskMs: Math.max(0, ...longTasks),
        historicalReadMs,
        historicalForkMs,
      };
    }, branchCount);
    report.benchmarks.push(measurement);
    assert.ok(
      measurement.p95Ms < 125,
      `${branchCount} colonies exceeded the 8 Hz worker budget`,
    );
    assert.ok(
      Math.max(...measurement.historicalReadMs) < 100,
      "Historical read exceeded 100 ms",
    );
    if (measurement.historicalForkMs !== null)
      assert.ok(
        measurement.historicalForkMs < 100,
        "Historical fork exceeded 100 ms",
      );
    assert.ok(
      measurement.frames > 5,
      "Main thread stopped rendering during worker benchmark",
    );
    console.log(JSON.stringify(measurement));
  }
  report.checks.push(
    "1,000-generation history retention at 6 and 12 branches",
    "historical fork after 1,000 generations",
    "branch and generation bounds",
  );
  assert.deepEqual(errors, []);
  report.passed = true;
} catch (error) {
  report.passed = false;
  report.error = error.stack;
  throw error;
} finally {
  await mkdir(new URL("../artifacts/", import.meta.url), { recursive: true });
  await writeFile(
    new URL("../artifacts/browser-feasibility.json", import.meta.url),
    JSON.stringify(report, null, 2) + "\n",
  );
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
}
console.log(
  "Browser feasibility passed. Results: artifacts/browser-feasibility.json",
);
