import {
  decode,
  encode,
  emptyBoard,
  flip,
  genesis,
  perturbation,
  step,
  divergence,
} from "./life.mjs";

const BOARD_KEY = btoa("board");
const MAX_BRANCHES = 12;
const MAX_CHECKPOINTS = 1200;
const SEGMENT_WRITES = 1536;

// Only used in a worker. Strata owns history and forks; JS keeps lightweight
// checkpoint coordinates and the current boards used by the Life stepper.
export class ColoniesEngine {
  constructor(
    createSession,
    {
      width = 64,
      height = 48,
      seed = 42,
      historyLimit = MAX_CHECKPOINTS,
      segmentWrites = SEGMENT_WRITES,
    } = {},
  ) {
    emptyBoard(width, height);
    if (
      !Number.isInteger(historyLimit) ||
      historyLimit < 2 ||
      historyLimit > MAX_CHECKPOINTS
    )
      throw new Error("History window must contain 2–1200 checkpoints.");
    if (
      !Number.isInteger(segmentWrites) ||
      segmentWrites < 1 ||
      segmentWrites > SEGMENT_WRITES
    )
      throw new Error("Storage segments must contain 1–1536 writes.");
    this.createSession = createSession;
    this.historyLimit = historyLimit;
    this.segmentWrites = segmentWrites;
    this.stores = new Map();
    this.nextStore = 0;
    this.activeStore = this.createStore();
    this.width = width;
    this.height = height;
    this.seed = seed;
    this.branches = new Map();
    this.failed = false;
    this.comparisonCache = new Map();
  }

  createStore() {
    const store = {
      id: this.nextStore++,
      session: this.createSession(),
      writes: 0,
      branches: new Set(),
    };
    this.stores.set(store.id, store);
    return store;
  }

  writingStore() {
    if (this.activeStore.writes >= this.segmentWrites)
      this.activeStore = this.createStore();
    return this.activeStore;
  }

  collectHistory() {
    const retained = new Set([this.activeStore.id]);
    for (const branch of this.branches.values())
      for (const checkpoint of branch.checkpoints)
        retained.add(checkpoint.store);
    // Free entire expired database segments, including their MVCC versions and
    // event rows. Trimming the JS index alone would leave WASM storage growing.
    for (const [id, store] of this.stores)
      if (!retained.has(id)) {
        store.session.free();
        this.stores.delete(id);
      }
  }

  command(command, store = this.activeStore) {
    const envelope = JSON.parse(store.session.execute(JSON.stringify(command)));
    if (envelope.error)
      throw new Error(`${envelope.error.code}: ${envelope.error.message}`);
    return envelope.data;
  }

  branch(name) {
    const branch = this.branches.get(name);
    if (!branch) throw new Error(`Unknown colony: ${name}`);
    return branch;
  }

  checkpoint(branch, id) {
    if (id === undefined) return branch.checkpoints.at(-1);
    const checkpoint = branch.checkpoints.find((c) => c.id === id);
    if (!checkpoint)
      throw new Error(
        `This moment is outside ${branch.name}'s recent history.`,
      );
    return checkpoint;
  }

  writable() {
    if (this.failed)
      throw new Error(
        "A database write failed. Start a new session before continuing.",
      );
  }

  persist(
    branch,
    board,
    generation,
    revision,
    kind,
    changes = [],
    store = this.writingStore(),
  ) {
    const metadata = {
      generation,
      revision,
      kind,
      changes,
      parent: branch.parent,
      width: this.width,
      height: this.height,
    };
    // Three separate commits, just like the native app. Only publish the
    // checkpoint after the final event succeeds. No atomicity is implied.
    let kv, event;
    try {
      if (!store.branches.has(branch.name)) {
        this.command({ type: "branch_create", branch: branch.name }, store);
        store.branches.add(branch.name);
      }
      kv = this.command(
        {
          type: "kv_put",
          branch: branch.name,
          key: BOARD_KEY,
          value: encode(board),
        },
        store,
      );
      this.command(
        {
          type: "json_set",
          branch: branch.name,
          key: "colony",
          path: "$",
          value: metadata,
        },
        store,
      );
      event = this.command(
        {
          type: "event_append",
          branch: branch.name,
          event_type: "checkpoint",
          payload: { ...metadata, boardVersion: kv.commit.version },
        },
        store,
      );
    } catch (error) {
      this.failed = true;
      throw error;
    }
    const checkpoint = {
      id: `${branch.name}:${store.id}:${event.commit.version}`,
      store: store.id,
      ordinal: (branch.nextOrdinal ??= 0),
      generation,
      revision,
      kind,
      changes,
      version: event.commit.version,
      timestamp: event.commit.timestamp,
      boardVersion: kv.commit.version,
      sequence: event.sequence,
    };
    branch.board = board;
    branch.nextOrdinal++;
    store.writes++;
    branch.checkpoints.push(checkpoint);
    if (branch.checkpoints.length > this.historyLimit)
      branch.checkpoints.splice(
        0,
        branch.checkpoints.length - this.historyLimit,
      );
    return checkpoint;
  }

  initialize() {
    this.command({
      type: "branch_fork_current",
      source: "default",
      branch: "control",
    });
    this.activeStore.branches.add("control");
    const control = { name: "control", parent: null, checkpoints: [] };
    this.persist(
      control,
      genesis(this.width, this.height, this.seed),
      0,
      0,
      "genesis",
    );
    this.branches.set(control.name, control);
    const used = new Set();
    for (let i = 1; i <= 5; i++) {
      const name = `experiment-${i}`;
      this.fork({ source: "control", name });
      const cell = perturbation(
        control.board,
        this.width,
        this.height,
        i,
        used,
      );
      used.add(cell.join(","));
      this.mutate({ name, cells: [cell] });
    }
    return this.snapshot();
  }

  snapshot() {
    return {
      width: this.width,
      height: this.height,
      seed: this.seed,
      limits: {
        branches: MAX_BRANCHES,
        checkpoints: this.historyLimit,
      },
      branches: [...this.branches.values()].map((branch) => ({
        name: branch.name,
        parent: branch.parent,
        parentAvailable:
          !!branch.parent &&
          this.branch(branch.parent.name).checkpoints.some(
            (c) => c.id === branch.parent.checkpoint,
          ),
        board: encode(branch.board),
        head: this.checkpoint(branch),
        checkpointCount: branch.checkpoints.length,
        oldestOrdinal: branch.checkpoints[0].ordinal,
      })),
    };
  }

  history({ name }) {
    return this.branch(name).checkpoints;
  }

  read({ name, checkpoint: id }) {
    const branch = this.branch(name);
    const checkpoint = this.checkpoint(branch, id);
    const store = this.stores.get(checkpoint.store);
    const value = this.command(
      {
        type: "kv_get",
        branch: name,
        key: BOARD_KEY,
        as_of: checkpoint.timestamp,
      },
      store,
    );
    if (!value.found || value.value.version !== checkpoint.boardVersion) {
      throw new Error("The requested board version is unavailable.");
    }
    const status = this.command(
      {
        type: "json_get",
        branch: name,
        key: "colony",
        path: "$",
        as_of: checkpoint.timestamp,
      },
      store,
    );
    if (
      !status.found ||
      status.value.value.generation !== checkpoint.generation ||
      status.value.value.revision !== checkpoint.revision
    ) {
      throw new Error("Board and status do not describe the same checkpoint.");
    }
    return {
      name,
      checkpoint,
      board: value.value.value,
      metadata: status.value.value,
    };
  }

  fork({ source, checkpoint: id, name }) {
    this.writable();
    if (this.branches.size >= MAX_BRANCHES)
      throw new Error("This session has reached its 12-colony limit.");
    if (
      !/^[a-z][a-z0-9-]{0,39}$/.test(name) ||
      name === "default" ||
      this.branches.has(name)
    )
      throw new Error(
        "Choose a unique colony name using lowercase letters, numbers, and hyphens.",
      );
    const parent = this.branch(source);
    const checkpoint = this.checkpoint(parent, id);
    const restored = this.read({ name: source, checkpoint: checkpoint.id });
    const store = this.stores.get(checkpoint.store);
    this.command(
      {
        type: "branch_fork_at_version",
        source,
        branch: name,
        version: checkpoint.version,
      },
      store,
    );
    store.branches.add(name);
    // Prove that the native fork itself inherited the past board before writing
    // any child data. Reconstructing a branch by copying a JS board is not a fork.
    const inherited = this.command(
      {
        type: "kv_get",
        branch: name,
        key: BOARD_KEY,
      },
      store,
    );
    if (!inherited.found || inherited.value.value !== restored.board) {
      this.failed = true;
      throw new Error("Strata fork did not inherit the requested checkpoint.");
    }
    const branch = {
      name,
      parent: {
        name: source,
        checkpoint: checkpoint.id,
        generation: checkpoint.generation,
        revision: checkpoint.revision,
        version: checkpoint.version,
      },
      checkpoints: [],
    };
    this.persist(
      branch,
      decode(restored.board),
      checkpoint.generation,
      0,
      "fork",
      [],
      store,
    );
    this.branches.set(name, branch);
    this.collectHistory();
    return this.snapshot();
  }

  mutate({ name, cells }) {
    this.writable();
    if (name === "control")
      throw new Error("Fork the control before editing it.");
    const branch = this.branch(name);
    if (
      !Array.isArray(cells) ||
      cells.length === 0 ||
      cells.length > this.width * this.height
    )
      throw new Error("Provide a non-empty list of cells.");
    const board = branch.board.slice();
    for (const cell of cells) {
      if (!Array.isArray(cell) || cell.length !== 2)
        throw new Error("Each cell needs x and y coordinates.");
      const [x, y] = cell;
      if (
        !Number.isInteger(x) ||
        !Number.isInteger(y) ||
        x < 0 ||
        y < 0 ||
        x >= this.width ||
        y >= this.height
      )
        throw new Error("Cell is outside the board.");
      flip(board, this.width, x, y);
    }
    const head = this.checkpoint(branch);
    this.persist(
      branch,
      board,
      head.generation,
      head.revision + 1,
      "mutation",
      cells,
    );
    this.collectHistory();
    return this.snapshot();
  }

  step({ names = [...this.branches.keys()] } = {}) {
    this.writable();
    if (
      !Array.isArray(names) ||
      names.length === 0 ||
      new Set(names).size !== names.length
    )
      throw new Error("Choose distinct colonies to step.");
    const branches = names.map((name) => this.branch(name));
    for (const branch of branches) {
      this.persist(
        branch,
        step(branch.board, this.width, this.height),
        this.checkpoint(branch).generation + 1,
        0,
        "tick",
      );
    }
    this.collectHistory();
    return this.snapshot();
  }

  compare({ left, right, generation }) {
    if (!Number.isInteger(generation) || generation < 0)
      throw new Error("Choose a simulation generation.");
    const at = (name) => {
      const checkpoints = this.branch(name).checkpoints;
      const checkpoint = checkpoints.findLast(
        (c) => c.generation === generation,
      );
      if (!checkpoint)
        throw new Error(
          `${name} has no checkpoint at generation ${generation}.`,
        );
      return this.read({ name, checkpoint: checkpoint.id });
    };
    const a = at(left),
      b = at(right);
    return {
      generation,
      left: a,
      right: b,
      divergence: divergence(decode(a.board), decode(b.board)),
    };
  }

  comparisonHistory({ left, right, leftHead, rightHead }) {
    const a = this.branch(left),
      b = this.branch(right);
    // The chart uses the latest saved revision of each generation. Cache only
    // derived counts keyed by immutable checkpoint IDs, never guessed boards.
    const latest = (branch, id) => {
      const head = this.checkpoint(branch, id);
      return new Map(
        branch.checkpoints
          .slice(0, branch.checkpoints.indexOf(head) + 1)
          .map((c) => [c.generation, c]),
      );
    };
    const leftAt = latest(a, leftHead),
      rightAt = latest(b, rightHead),
      points = [];
    for (const [generation, checkpoint] of leftAt) {
      const other = rightAt.get(generation);
      if (!other) continue;
      const key = `${checkpoint.id}|${other.id}`;
      let difference = this.comparisonCache.get(key);
      if (difference === undefined) {
        const mine = this.read({ name: left, checkpoint: checkpoint.id });
        const theirs = this.read({ name: right, checkpoint: other.id });
        difference = divergence(decode(mine.board), decode(theirs.board));
        this.comparisonCache.set(key, difference);
        if (this.comparisonCache.size > 4096)
          this.comparisonCache.delete(this.comparisonCache.keys().next().value);
      }
      points.push({
        generation,
        divergence: difference,
        leftRevision: checkpoint.revision,
        rightRevision: other.revision,
      });
    }
    return { left, right, points };
  }

  dispose() {
    for (const store of this.stores.values()) store.session.free();
    this.stores.clear();
  }
}
