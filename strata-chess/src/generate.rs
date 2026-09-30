//! Positions to solve, made rather than collected.
//!
//! Five hand-picked positions is five. The generator makes a legal position to
//! a recipe, and the caller asks Stockfish whether it is any good: this side
//! knows what is legal, the engine knows what is interesting, and neither can
//! answer the other's question.
//!
//! What makes a position a puzzle is not that it is winnable but that it has
//! *one* answer. That is a MultiPV question - the gap between the best move and
//! the second best - and it is asked in the browser, where the engine lives.
//! A position with three moves that all win is a position, not a puzzle.

use cozy_chess::{Board, Color, File, Piece, Rank, Square};

/// What goes on the board for a given level.
///
/// Sparse endings, because they are decisive far more often than a random
/// middlegame and they are where a single move is most often the only move.
struct Recipe {
    white: &'static [Piece],
    black: &'static [Piece],
}

fn recipe(level: &str) -> Recipe {
    match level {
        // A lone king and enough to mate him. Most of these are mate in one or
        // two, which is what makes them the first rung.
        "beginner" => Recipe {
            white: &[Piece::Queen, Piece::Rook],
            black: &[],
        },
        "easy" => Recipe {
            white: &[Piece::Rook, Piece::Rook],
            black: &[Piece::Pawn],
        },
        // Enough material on both sides that the answer is a tactic rather
        // than a mating net.
        "medium" => Recipe {
            white: &[Piece::Rook, Piece::Knight, Piece::Pawn],
            black: &[Piece::Rook, Piece::Pawn],
        },
        // Technique: few pieces, long win, every move load-bearing.
        "hard" => Recipe {
            white: &[Piece::Rook, Piece::Pawn],
            black: &[Piece::Rook],
        },
        // The defender's side of a pawn ending: you have nothing, they have a
        // pawn, and there is usually exactly one square that holds. Two pawns
        // each was the first try and almost never produced a knife edge - most
        // random pawn endings are comfortably won or comfortably drawn, and
        // 200 seeds running found nothing at all. Taking material away
        // sharpened it, which is the opposite of the obvious move.
        _ => Recipe {
            white: &[],
            black: &[Piece::Pawn],
        },
    }
}

/// A small deterministic generator, so a seed names a position exactly and the
/// browser can ask for "another" by counting up rather than by luck.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

fn square_of(index: u32) -> Square {
    let file = File::index(index as usize % 8);
    let rank = Rank::index(index as usize / 8);
    Square::new(file, rank)
}

fn kings_touch(a: Square, b: Square) -> bool {
    let df = (a.file() as i8 - b.file() as i8).abs();
    let dr = (a.rank() as i8 - b.rank() as i8).abs();
    df <= 1 && dr <= 1
}

/// A legal position built to the level's recipe, or `None` if this seed did
/// not produce one. The caller tries the next seed; nothing here loops, so a
/// hostile recipe cannot hang the page.
pub fn candidate(level: &str, seed: u32) -> Option<String> {
    let mut rng = Rng(seed.wrapping_mul(2_654_435_761).wrapping_add(1));
    let recipe = recipe(level);

    let mut used = [false; 64];
    let mut board: Vec<(Square, Color, Piece)> = Vec::new();

    let mut take = |rng: &mut Rng, used: &mut [bool; 64]| -> Option<Square> {
        for _ in 0..64 {
            let index = rng.below(64);
            if !used[index as usize] {
                used[index as usize] = true;
                return Some(square_of(index));
            }
        }
        None
    };

    let white_king = take(&mut rng, &mut used)?;
    let black_king = take(&mut rng, &mut used)?;
    // Kings that touch is the one illegality this placement can produce on its
    // own, and it is cheaper to reject the seed than to shuffle.
    if kings_touch(white_king, black_king) {
        return None;
    }
    board.push((white_king, Color::White, Piece::King));
    board.push((black_king, Color::Black, Piece::King));

    for (colour, pieces) in [(Color::White, recipe.white), (Color::Black, recipe.black)] {
        for piece in pieces {
            let square = take(&mut rng, &mut used)?;
            // A pawn on the first or last rank is not a position anyone can be
            // handed, so the seed is spent rather than the pawn moved.
            if *piece == Piece::Pawn
                && (square.rank() == Rank::First || square.rank() == Rank::Eighth)
            {
                return None;
            }
            board.push((square, colour, *piece));
        }
    }

    let fen = to_fen(&board);
    let parsed = Board::from_fen(&fen, false).ok()?;
    // The side not to move must not already be in check: that is a position
    // arrived at by an illegal move, and no engine should be asked about it.
    if !parsed.checkers().is_empty() {
        return None;
    }
    // And there has to be something to play.
    let mut any = false;
    parsed.generate_moves(|_| {
        any = true;
        true
    });
    any.then_some(fen)
}

fn to_fen(pieces: &[(Square, Color, Piece)]) -> String {
    let mut squares: [Option<(Color, Piece)>; 64] = [None; 64];
    for (square, colour, piece) in pieces {
        squares[*square as usize] = Some((*colour, *piece));
    }

    let mut rows = Vec::with_capacity(8);
    for rank in (0..8).rev() {
        let mut row = String::new();
        let mut gap = 0;
        for file in 0..8 {
            match squares[rank * 8 + file] {
                None => gap += 1,
                Some((colour, piece)) => {
                    if gap > 0 {
                        row.push_str(&gap.to_string());
                        gap = 0;
                    }
                    let letter = match piece {
                        Piece::Pawn => 'p',
                        Piece::Knight => 'n',
                        Piece::Bishop => 'b',
                        Piece::Rook => 'r',
                        Piece::Queen => 'q',
                        Piece::King => 'k',
                    };
                    row.push(if colour == Color::White {
                        letter.to_ascii_uppercase()
                    } else {
                        letter
                    });
                }
            }
        }
        if gap > 0 {
            row.push_str(&gap.to_string());
        }
        rows.push(row);
    }
    // White to move, no castling, no en passant: a position handed to someone
    // has no history to inherit rights from.
    format!("{} w - - 0 1", rows.join("/"))
}
