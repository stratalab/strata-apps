// Packed row-major toroidal Life, matching src/life.rs and src/patterns.rs.
export function emptyBoard(width, height) {
  if (
    !Number.isInteger(width) ||
    !Number.isInteger(height) ||
    width < 8 ||
    height < 8 ||
    width > 128 ||
    height > 128
  ) {
    throw new Error("Board dimensions must be integers between 8 and 128.");
  }
  return new Uint8Array(Math.ceil((width * height) / 8));
}

export function live(board, width, x, y) {
  const bit = y * width + x;
  return (board[bit >> 3] >> (bit & 7)) & 1;
}

export function flip(board, width, x, y) {
  const bit = y * width + x;
  board[bit >> 3] ^= 1 << (bit & 7);
}

export function step(board, width, height) {
  const next = emptyBoard(width, height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let count = 0;
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          if (dx || dy)
            count += live(
              board,
              width,
              (x + dx + width) % width,
              (y + dy + height) % height,
            );
        }
      }
      if (count === 3 || (count === 2 && live(board, width, x, y)))
        flip(next, width, x, y);
    }
  }
  return next;
}

export function encode(board) {
  return btoa(String.fromCharCode(...board));
}
export function decode(board) {
  return Uint8Array.from(atob(board), (c) => c.charCodeAt(0));
}
export function divergence(a, b) {
  let count = 0;
  for (let i = 0; i < a.length; i++) {
    let bits = a[i] ^ b[i];
    while (bits) {
      count++;
      bits &= bits - 1;
    }
  }
  return count;
}

function mix(value) {
  value = BigInt.asUintN(64, value ^ (value << 13n));
  value ^= value >> 7n;
  return BigInt.asUintN(64, value ^ (value << 17n));
}

export function genesis(width, height, seed) {
  const board = emptyBoard(width, height);
  let state = mix(BigInt(seed) ^ 0x9e3779b97f4a7c15n);
  const mx = Math.floor(width / 6),
    my = Math.floor(height / 6);
  for (let y = my; y < height - my; y++) {
    for (let x = mx; x < width - mx; x++) {
      state = mix(state);
      if (state % 5n < 2n) flip(board, width, x, y);
    }
  }
  const set = (x, y) => {
    if (x < width && y < height && !live(board, width, x, y))
      flip(board, width, x, y);
  };
  for (const [dx, dy] of [
    [1, 0],
    [2, 1],
    [0, 2],
    [1, 2],
    [2, 2],
  ])
    set(2 + dx, 2 + dy);
  for (const [dx, dy] of [
    [1, 0],
    [2, 0],
    [0, 1],
    [1, 1],
    [1, 2],
  ])
    set(Math.floor(width / 2) + 3 + dx, Math.floor(height / 2) + 2 + dy);
  return board;
}

export function perturbation(board, width, height, index, used) {
  const sx = (index * 7 + 13) % width,
    sy = (index * 11 + 5) % height;
  for (let radius = 0; radius < Math.max(width, height); radius++) {
    for (let dy = -radius; dy <= radius; dy++) {
      for (let dx = -radius; dx <= radius; dx++) {
        if (Math.abs(dx) !== radius && Math.abs(dy) !== radius) continue;
        const x = (((sx + dx) % width) + width) % width,
          y = (((sy + dy) % height) + height) % height;
        if (used.has(`${x},${y}`) || live(board, width, x, y)) continue;
        for (let ny = -1; ny <= 1; ny++) {
          for (let nx = -1; nx <= 1; nx++) {
            if (
              (nx || ny) &&
              live(
                board,
                width,
                (x + nx + width) % width,
                (y + ny + height) % height,
              )
            )
              return [x, y];
          }
        }
      }
    }
  }
  throw new Error("No unused mutation site found.");
}
