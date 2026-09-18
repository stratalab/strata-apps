import assert from "node:assert/strict";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import init, { StrataSession } from "../web/pkg/strata_wasm.js";
import { ColoniesEngine } from "../web/engine.mjs";
import { encode, step } from "../web/life.mjs";

const wasm = await init({
  module_or_path: await readFile(
    new URL("../web/pkg/strata_wasm_bg.wasm", import.meta.url),
  ),
});
let allocated = 0,
  released = 0;
const createSession = () => {
  const session = new StrataSession();
  allocated++;
  return {
    execute: (command) => session.execute(command),
    free() {
      session.free();
      released++;
    },
  };
};
const checks = [],
  memory = [];
const engine = new ColoniesEngine(createSession);
try {
  const initial = engine.initialize().branches[1];
  // Run through several complete default windows, including physical database
  // retirement. Check every retained board against an independent Life stepper.
  let expected = engine.branch("experiment-1").board.slice();
  const boards = new Map([[initial.head.id, encode(expected)]]);
  for (let generation = 1; generation <= 5000; generation++) {
    expected = step(expected, 64, 48);
    const snapshot = engine.step();
    const current = snapshot.branches[1];
    assert.equal(current.board, encode(expected));
    assert.equal(current.head.generation, generation);
    assert.ok(snapshot.branches.every((b) => b.checkpointCount <= 1200));
    boards.set(current.head.id, current.board);
    if (boards.size > 1200) boards.delete(boards.keys().next().value);
    if (generation % 1000 === 0) {
      for (const checkpoint of engine.history({ name: "experiment-1" })) {
        if (boards.has(checkpoint.id))
          assert.equal(
            engine.read({ name: "experiment-1", checkpoint: checkpoint.id })
              .board,
            boards.get(checkpoint.id),
          );
      }
      memory.push({
        generation,
        bytes: wasm.memory.buffer.byteLength,
        stores: engine.stores.size,
      });
    }
  }
  assert.ok(released > 0, "Old database sessions must actually be freed");
  assert.ok(engine.stores.size <= 6, "Expired storage must not accumulate");
  // WASM cannot shrink its linear memory, but freed segments must be reusable.
  assert.ok(
    memory.at(-1).bytes <= memory[2].bytes * 1.25,
    "Memory should plateau after warm-up",
  );
  assert.throws(
    () => engine.read({ name: "experiment-1", checkpoint: initial.head.id }),
    /outside.*recent history/,
  );
  assert.throws(
    () =>
      engine.fork({
        source: "experiment-1",
        checkpoint: initial.head.id,
        name: "expired",
      }),
    /outside.*recent history/,
  );
  const parentBefore = engine.snapshot().branches[1];
  const oldest = engine.history({ name: "experiment-1" })[0];
  const oldBoard = engine.read({
    name: "experiment-1",
    checkpoint: oldest.id,
  }).board;
  const child = engine
    .fork({ source: "experiment-1", checkpoint: oldest.id, name: "fork-1" })
    .branches.at(-1);
  assert.equal(child.board, oldBoard);
  assert.equal(child.parent.checkpoint, oldest.id);
  assert.deepEqual(engine.snapshot().branches[1], parentBefore);
  assert.equal(child.parentAvailable, true);
  engine.step();
  assert.equal(engine.snapshot().branches.at(-1).parentAvailable, false);
  // The source moment expires; the independently running child must survive.
  for (let i = 0; i < 1300; i++) engine.step();
  assert.equal(engine.snapshot().branches.at(-1).checkpointCount, 1200);
  for (const branch of engine.snapshot().branches) {
    const history = engine.history({ name: branch.name });
    for (const checkpoint of [history[0], history.at(-1)])
      assert.equal(
        engine.read({ name: branch.name, checkpoint: checkpoint.id }).checkpoint
          .id,
        checkpoint.id,
      );
  }
  const comparison = engine.comparisonHistory({
    left: "experiment-1",
    right: "control",
  });
  assert.equal(comparison.points.length, 1200);
  checks.push(
    "6,301 generations without a stop; 1,200 retained checkpoints per colony",
  );
  checks.push(
    "Retained history matches Life across segment boundaries; expired reads/forks reject",
  );
  checks.push(
    "Exact native fork at the oldest moment; child survives source history and storage expiration",
  );
  checks.push(
    "Old database sessions freed; WASM allocation plateaus across repeated windows",
  );
} finally {
  engine.dispose();
}
assert.equal(allocated, released, "Dispose must free every remaining database");

// Heavy edits at one generation and uneven branch advancement must also trim.
const edits = new ColoniesEngine(createSession, {
  historyLimit: 8,
  segmentWrites: 12,
});
try {
  const initial = edits.initialize();
  const untouched = initial.branches[2];
  for (let i = 0; i < 100; i++)
    edits.mutate({
      name: "experiment-1",
      cells: [
        [0, 0],
        [1, 1],
      ],
    });
  const history = edits.history({ name: "experiment-1" });
  assert.equal(history.length, 8);
  assert.equal(history.at(-1).generation, 0);
  assert.equal(history.at(-1).revision, 101);
  assert.equal(
    edits.read({ name: untouched.name, checkpoint: untouched.head.id }).board,
    untouched.board,
  );
  const fork = edits
    .fork({ source: "experiment-1", checkpoint: history[0].id, name: "fork-1" })
    .branches.at(-1);
  const before = edits.snapshot().branches[1];
  edits.mutate({ name: fork.name, cells: [[2, 2]] });
  assert.deepEqual(edits.snapshot().branches[1], before);
  assert.ok(edits.stores.size < 5);
  checks.push(
    "Mutation-only windows, idle colonies, same-generation revisions, and child edits remain isolated",
  );
} finally {
  edits.dispose();
}
assert.equal(allocated, released);
await mkdir("artifacts", { recursive: true });
await writeFile(
  "artifacts/retention-verification.json",
  JSON.stringify({ checks, memory, allocated, released }, null, 2) + "\n",
);
console.log(checks.map((check) => `PASS: ${check}`).join("\n"));
console.log(JSON.stringify(memory));
