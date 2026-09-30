// Making positions worth solving.
//
// The Rust side hands out legal positions to a recipe; this side asks the
// engine whether one is any good. What makes a position a puzzle is that it has
// one answer, so the test is the gap between the best move and the second best,
// and the goal is read off what the engine actually found rather than declared
// in advance.

/** In centipawns; a mate is worth more than any material. */
export function toCp(found) {
  if (!found) return 0;
  if (found.kind === 'mate') return found.value > 0 ? 10000 - found.value : -10000 - found.value;
  return found.value;
}

/**
 * Is this position the kind of thing the level promises, and if so, what is
 * the visitor being asked to do?
 */
export function judge(level, top) {
  const { first, second } = top;
  if (!first) return null;
  const best = toCp(first);
  // A position with only one legal move is not a puzzle, it is a formality.
  const other = second ? toCp(second) : -20000;
  const gap = best - other;

  const mate = first.kind === 'mate' && first.value > 0 ? first.value : null;

  if (level === 'beginner') {
    if (mate !== null && mate <= 2) {
      return { goal: mate === 1 ? 'Mate in one' : 'Mate in two', win: { kind: 'mate' } };
    }
    return null;
  }
  if (level === 'easy') {
    if (mate !== null && mate <= 4) {
      return { goal: `Mate in ${mate}`, win: { kind: 'mate' } };
    }
    return null;
  }
  if (level === 'medium') {
    if (mate !== null && mate <= 6) return { goal: `Mate in ${mate}`, win: { kind: 'mate' } };
    if (best >= 250 && gap >= 200) {
      return { goal: 'Win material', win: { kind: 'material', cp: 200 } };
    }
    return null;
  }
  if (level === 'hard') {
    // A win that other moves throw away: the definition of technique.
    if (best >= 200 && gap >= 150) {
      return { goal: 'Convert the advantage', win: { kind: 'material', cp: 300 } };
    }
    if (mate !== null) return { goal: `Mate in ${mate}`, win: { kind: 'mate' } };
    return null;
  }
  // Grandmaster: the only move holds, and everything else loses. This is the
  // shape Réti's study has, and it is the one worth hunting for.
  if (Math.abs(best) <= 100 && other <= -200) {
    return { goal: 'Hold the draw', win: { kind: 'hold', plies: 10, band: 150 } };
  }
  if (best >= 300 && gap >= 350) {
    return { goal: 'Find the only win', win: { kind: 'material', cp: 300 } };
  }
  return null;
}

/** What is on the board, as a name: "Rook and pawn", "Two rooks". */
function nameFor(fen) {
  const WORD = { q: 'queen', r: 'rook', b: 'bishop', n: 'knight', p: 'pawn' };
  const ORDER = ['queen', 'rook', 'bishop', 'knight', 'pawn'];
  const mine = [];
  for (const ch of fen.split(' ')[0]) {
    const word = WORD[ch.toLowerCase()];
    if (word && ch === ch.toUpperCase()) mine.push(word);
  }
  // Heaviest first, so the same material is always called the same thing
  // rather than named in whatever order the rows happen to run.
  mine.sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
  if (mine.length === 0) return 'Bare king';
  const counted = new Map();
  for (const word of mine) counted.set(word, (counted.get(word) ?? 0) + 1);
  const parts = [...counted].map(([word, n]) =>
    n === 1 ? word : n === 2 ? `two ${word}s` : `${n} ${word}s`,
  );
  const text = parts.length === 1 ? parts[0] : `${parts.slice(0, -1).join(', ')} and ${parts.at(-1)}`;
  return text[0].toUpperCase() + text.slice(1);
}

const BRIEF = {
  beginner: 'A king with nowhere to go. Find the move that ends it.',
  easy: 'There is a mate here. It takes more than one move to see it.',
  medium: 'One move is worth much more than the rest. Find it.',
  hard: 'The win is there. Most moves let it go.',
  grandmaster: 'Only one move saves this. Everything else loses.',
};

/**
 * Hunts for a position at `level`, trying seeds until the engine agrees.
 *
 * `onTry` is called for each attempt so the page can say it is working rather
 * than appearing to have frozen.
 */
export async function findPosition({ chess, engine, level, from = 1, depth = 14, tries = 120, onTry }) {
  let seed = from;
  for (let attempt = 0; attempt < tries; attempt++, seed++) {
    const fen = chess.candidate(level, seed);
    if (!fen) continue;
    onTry?.(attempt + 1);
    const top = await engine.top2(fen, depth);
    const verdict = judge(level, top);
    if (!verdict) continue;
    return {
      id: `${level}-${seed}`,
      level,
      title: nameFor(fen),
      brief: BRIEF[level],
      fen,
      you: 'w',
      goal: verdict.goal,
      win: verdict.win,
      hint: `The engine likes ${top.first.move}.`,
      seed,
    };
  }
  return null;
}
