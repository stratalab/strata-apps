//! The rules, and the two places they are easy to get wrong.
//!
//! Move generation is `cozy-chess`, which is MIT and perft-verified, because a
//! move generator is where a bug is both fatal and invisible: it produces a
//! position that looks fine and is not legal, and nothing in the UI can tell.
//! What this module owns is the two translations between that library and a
//! board a person is looking at.
//!
//! 1. **Castling.** `cozy-chess` speaks Chess960, where castling is encoded as
//!    the king taking its own rook - e1h1, not e1g1. A player dragging a king
//!    two squares means e1g1, so the two forms are converted at this boundary
//!    and nowhere else.
//! 2. **SAN.** The library deals in squares; a move list wants `Nf3`, `exd5`,
//!    `O-O`, `Qh4#`. That includes the disambiguation rules, which is the part
//!    people usually skip and then print `Nd2` for a position with two knights
//!    that can both reach d2.

use cozy_chess::{Board, Color, File, GameStatus, Move, Piece, Rank, Square};

/// A move in the terms a board speaks: where it started, where it landed, and
/// what a pawn became.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerMove {
    pub from: Square,
    pub to: Square,
    pub promotion: Option<Piece>,
}

/// True when this king move is a castle written the way a player plays it:
/// two squares sideways.
fn is_player_castle(board: &Board, mv: PlayerMove) -> bool {
    board.piece_on(mv.from) == Some(Piece::King)
        && (mv.from.file() as i8 - mv.to.file() as i8).abs() == 2
}

/// A player's e1g1 becomes the library's e1h1.
pub fn to_engine(board: &Board, mv: PlayerMove) -> Move {
    let mut to = mv.to;
    if is_player_castle(board, mv) {
        let rook_file = if mv.to.file() > mv.from.file() {
            File::H
        } else {
            File::A
        };
        to = Square::new(rook_file, mv.from.rank());
    }
    Move {
        from: mv.from,
        to,
        promotion: mv.promotion,
    }
}

/// The library's e1h1 becomes a player's e1g1.
pub fn to_player(board: &Board, mv: Move) -> PlayerMove {
    let mut to = mv.to;
    // A king landing on its own rook is the library's castle encoding. Any
    // other king move, including a capture, is left alone.
    if board.piece_on(mv.from) == Some(Piece::King)
        && board.color_on(mv.to) == Some(board.side_to_move())
        && board.piece_on(mv.to) == Some(Piece::Rook)
    {
        let file = if mv.to.file() > mv.from.file() {
            File::G
        } else {
            File::C
        };
        to = Square::new(file, mv.from.rank());
    }
    PlayerMove {
        from: mv.from,
        to,
        promotion: mv.promotion,
    }
}

/// Every legal move, in player terms.
pub fn legal_moves(board: &Board) -> Vec<PlayerMove> {
    let mut out = Vec::new();
    board.generate_moves(|moves| {
        for mv in moves {
            out.push(to_player(board, mv));
        }
        false
    });
    out
}

/// Standard algebraic notation for a move that is legal in this position.
pub fn san(board: &Board, mv: Move) -> String {
    let piece = match board.piece_on(mv.from) {
        Some(piece) => piece,
        None => return String::from("--"),
    };

    let castle = piece == Piece::King
        && board.color_on(mv.to) == Some(board.side_to_move())
        && board.piece_on(mv.to) == Some(Piece::Rook);

    let mut text = if castle {
        if mv.to.file() > mv.from.file() {
            String::from("O-O")
        } else {
            String::from("O-O-O")
        }
    } else {
        let captures = board.color_on(mv.to) == Some(!board.side_to_move())
            || (piece == Piece::Pawn && Some(mv.to) == board.en_passant().map(|file| {
                Square::new(
                    file,
                    match board.side_to_move() {
                        Color::White => Rank::Sixth,
                        Color::Black => Rank::Third,
                    },
                )
            }));

        let mut text = String::new();
        if piece == Piece::Pawn {
            if captures {
                text.push(file_char(mv.from.file()));
                text.push('x');
            }
        } else {
            text.push(piece_char(piece));
            text.push_str(&disambiguate(board, mv, piece));
            if captures {
                text.push('x');
            }
        }
        text.push(file_char(mv.to.file()));
        text.push(rank_char(mv.to.rank()));
        if let Some(promotion) = mv.promotion {
            text.push('=');
            text.push(piece_char(promotion));
        }
        text
    };

    // Check and mate are properties of the position the move produces, so the
    // move has to be played to find out.
    let mut after = board.clone();
    after.play_unchecked(mv);
    if !after.checkers().is_empty() {
        text.push(if after.status() == GameStatus::Won {
            '#'
        } else {
            '+'
        });
    }
    text
}

/// The file, rank or both that a move needs to be unambiguous.
///
/// Printing `Nd2` when two knights can reach d2 is the classic SAN bug: the
/// notation is not wrong so much as unreadable, and it round-trips to the
/// wrong move.
fn disambiguate(board: &Board, mv: Move, piece: Piece) -> String {
    let mut same_file = false;
    let mut same_rank = false;
    let mut rivals = 0;

    board.generate_moves(|moves| {
        for other in moves {
            if other.to != mv.to || other.from == mv.from {
                continue;
            }
            if board.piece_on(other.from) != Some(piece) {
                continue;
            }
            rivals += 1;
            if other.from.file() == mv.from.file() {
                same_file = true;
            }
            if other.from.rank() == mv.from.rank() {
                same_rank = true;
            }
        }
        false
    });

    if rivals == 0 {
        return String::new();
    }
    // File alone if it tells them apart, then rank, then both.
    if !same_file {
        return file_char(mv.from.file()).to_string();
    }
    if !same_rank {
        return rank_char(mv.from.rank()).to_string();
    }
    format!("{}{}", file_char(mv.from.file()), rank_char(mv.from.rank()))
}

fn piece_char(piece: Piece) -> char {
    match piece {
        Piece::Pawn => 'P',
        Piece::Knight => 'N',
        Piece::Bishop => 'B',
        Piece::Rook => 'R',
        Piece::Queen => 'Q',
        Piece::King => 'K',
    }
}

fn file_char(file: File) -> char {
    (b'a' + file as u8) as char
}

fn rank_char(rank: Rank) -> char {
    (b'1' + rank as u8) as char
}

/// Counts leaf nodes to a depth. The standard way to say a move generator is
/// right: the numbers are published, and a generator that is wrong anywhere
/// misses them.
pub fn perft(board: &Board, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut nodes = 0;
    board.generate_moves(|moves| {
        for mv in moves {
            if depth == 1 {
                nodes += 1;
            } else {
                let mut next = board.clone();
                next.play_unchecked(mv);
                nodes += perft(&next, depth - 1);
            }
        }
        false
    });
    nodes
}
