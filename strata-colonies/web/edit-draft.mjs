import { decode, encode, live, flip } from "./life.mjs";

// Drafts never write to Strata. A gesture is one undo step; applying the draft
// sends only the net changed cells to the engine's grouped mutation operation.
export class EditDraft {
  constructor({ name, checkpoint, board, width, height }) {
    Object.assign(this, { name, checkpoint, width, height });
    this.original = decode(board);
    this.board = this.original.slice();
    this.undoStack = [];
    this.stroke = null;
  }
  begin(cell, tool = "auto") {
    this.stroke = {
      before: this.board.slice(),
      last: cell,
      value:
        tool === "auto"
          ? !live(this.board, this.width, ...cell)
          : tool === "paint",
    };
    this.move(cell);
  }
  move([x1, y1]) {
    if (!this.stroke) return;
    let [x, y] = this.stroke.last;
    const dx = Math.abs(x1 - x),
      dy = -Math.abs(y1 - y);
    const sx = x < x1 ? 1 : -1,
      sy = y < y1 ? 1 : -1;
    let error = dx + dy;
    for (;;) {
      if (!!live(this.board, this.width, x, y) !== this.stroke.value)
        flip(this.board, this.width, x, y);
      if (x === x1 && y === y1) break;
      const twice = 2 * error;
      if (twice >= dy) {
        error += dy;
        x += sx;
      }
      if (twice <= dx) {
        error += dx;
        y += sy;
      }
    }
    this.stroke.last = [x1, y1];
  }
  end(cancel = false) {
    if (!this.stroke) return;
    if (cancel) this.board = this.stroke.before;
    else if (this.board.some((byte, i) => byte !== this.stroke.before[i]))
      this.undoStack.push(this.stroke.before);
    this.stroke = null;
  }
  undo() {
    this.end();
    if (this.undoStack.length) this.board = this.undoStack.pop();
  }
  clear() {
    this.board = this.original.slice();
    this.undoStack = [];
    this.stroke = null;
  }
  get encoded() {
    return encode(this.board);
  }
  get changes() {
    const cells = [];
    for (let i = 0; i < this.width * this.height; i++) {
      const x = i % this.width,
        y = Math.floor(i / this.width);
      if (
        live(this.board, this.width, x, y) !==
        live(this.original, this.width, x, y)
      )
        cells.push([x, y]);
    }
    return cells;
  }
}
