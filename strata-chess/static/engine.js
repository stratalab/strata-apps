// Stockfish, over UCI, in a worker of its own.
//
// Stockfish is GPL-3.0 and is not part of this app: it is a separate program,
// started at runtime, talked to over postMessage. Nothing from it is linked
// into the WebAssembly this repository builds. The licence, the exact version
// and where to get the Corresponding Source are in stockfish/README.md, which
// is served from the same directory as the binary.
const PATH = './stockfish/stockfish-19-lite-single.js';

export class Engine {
  constructor() {
    this.worker = new Worker(PATH);
    this.queue = [];
    this.worker.onmessage = (event) => {
      const line = typeof event.data === 'string' ? event.data : String(event.data);
      for (const waiter of [...this.queue]) {
        const value = waiter.match(line);
        if (value !== null && value !== undefined) {
          this.queue.splice(this.queue.indexOf(waiter), 1);
          waiter.resolve(value);
        }
      }
    };
    this.worker.onerror = (event) => {
      for (const waiter of this.queue.splice(0)) {
        waiter.reject(new Error(event.message || 'engine failed'));
      }
    };
    this.lines = [];
    this.ready = this.handshake();
  }

  send(command) {
    this.worker.postMessage(command);
  }

  /** Resolves when `match` returns something for a line the engine printed. */
  await(match, timeout = 30000) {
    return new Promise((resolve, reject) => {
      const waiter = { match, resolve, reject };
      this.queue.push(waiter);
      setTimeout(() => {
        if (this.queue.includes(waiter)) {
          this.queue.splice(this.queue.indexOf(waiter), 1);
          reject(new Error('the engine did not answer'));
        }
      }, timeout);
    });
  }

  async handshake() {
    this.send('uci');
    await this.await((l) => (l === 'uciok' ? true : null));
    this.send('isready');
    await this.await((l) => (l === 'readyok' ? true : null));
    return true;
  }

  /**
   * Searches a position for the best two moves.
   *
   * The gap between them is what says whether a position is a puzzle. One good
   * move and one bad one is a question with an answer; three moves that all
   * win is just a position.
   */
  async top2(fen, depth) {
    await this.ready;
    const best = new Map();
    const done = this.await((line) => {
      const rank = line.match(/ multipv (\d+) /);
      const score = line.match(/score (cp|mate) (-?\d+)/);
      const move = line.match(/ pv ([a-h][1-8][a-h][1-8][qrbn]?)/);
      if (rank && score && move) {
        best.set(Number(rank[1]), {
          kind: score[1],
          value: Number(score[2]),
          move: move[1],
        });
      }
      return line.startsWith('bestmove') ? true : null;
    }, 60000);
    this.send('setoption name MultiPV value 2');
    this.send('ucinewgame');
    this.send(`position fen ${fen}`);
    this.send(`go depth ${depth}`);
    await done;
    this.send('setoption name MultiPV value 1');
    return { first: best.get(1) ?? null, second: best.get(2) ?? null };
  }

  /**
   * Searches a position and reports what it found.
   *
   * `score` is always from the point of view of the side to move, which is how
   * UCI defines it and the thing that is easiest to get backwards.
   */
  async analyse(fen, depth) {
    await this.ready;
    let last = null;
    const best = this.await((line) => {
      if (line.includes(' score ')) last = line;
      return line.startsWith('bestmove') ? line.split(' ')[1] : null;
    }, 60000);
    this.send('ucinewgame');
    this.send(`position fen ${fen}`);
    this.send(`go depth ${depth}`);
    const move = await best;
    const matched = last?.match(/score (cp|mate) (-?\d+)/);
    return {
      best: move,
      kind: matched?.[1] ?? 'cp',
      value: matched ? Number(matched[2]) : 0,
    };
  }
}
