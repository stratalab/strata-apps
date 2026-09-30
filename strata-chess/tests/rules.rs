//! Perft, and the two translations this crate owns.
//!
//! The perft numbers are the published ones. A move generator that is wrong
//! anywhere - one missing en-passant pin, one castle through check allowed -
//! misses them, which is why this is the test that says the rules are right
//! rather than a handful of positions somebody thought of.

use cozy_chess::{Board, Color, File, Piece, Rank, Square};
use strata_chess::game::{legal_moves, perft, san, to_engine, to_player, PlayerMove};

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
/// "Kiwipete": the standard second test position, dense with castling,
/// en passant and pins precisely because those are what break generators.
const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

#[test]
fn perft_from_the_start_position() {
    let board = Board::from_fen(START, false).expect("start fen");
    assert_eq!(perft(&board, 1), 20);
    assert_eq!(perft(&board, 2), 400);
    assert_eq!(perft(&board, 3), 8_902);
    assert_eq!(perft(&board, 4), 197_281);
}

#[test]
fn perft_from_kiwipete() {
    let board = Board::from_fen(KIWIPETE, false).expect("kiwipete fen");
    assert_eq!(perft(&board, 1), 48);
    assert_eq!(perft(&board, 2), 2_039);
    assert_eq!(perft(&board, 3), 97_862);
}

#[test]
fn perft_from_a_position_built_to_break_en_passant() {
    // Position 3 from the standard set: a rook-and-pawn ending where en
    // passant is legal and also pinned, which is the classic generator bug.
    let board = Board::from_fen("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", false).expect("fen");
    assert_eq!(perft(&board, 1), 14);
    assert_eq!(perft(&board, 2), 191);
    assert_eq!(perft(&board, 3), 2_812);
    assert_eq!(perft(&board, 4), 43_238);
}

#[test]
fn a_player_castles_two_squares_and_the_library_takes_its_rook() {
    // cozy-chess speaks Chess960: castling is king-takes-own-rook. A person
    // dragging a king from e1 to g1 means the same move, and this is the only
    // place the two spellings meet.
    let board = Board::from_fen(KIWIPETE, false).expect("fen");
    let player = PlayerMove {
        from: Square::E1,
        to: Square::G1,
        promotion: None,
    };
    let engine = to_engine(&board, player);
    assert_eq!(engine.to, Square::H1, "kingside castle targets the rook");

    let back = to_player(&board, engine);
    assert_eq!(back.to, Square::G1, "and comes back as the player's move");

    let queenside = to_engine(
        &board,
        PlayerMove {
            from: Square::E1,
            to: Square::C1,
            promotion: None,
        },
    );
    assert_eq!(queenside.to, Square::A1, "queenside targets the a-rook");
}

#[test]
fn legal_moves_are_offered_in_the_players_spelling() {
    let board = Board::from_fen(KIWIPETE, false).expect("fen");
    let moves = legal_moves(&board);
    assert!(
        moves
            .iter()
            .any(|m| m.from == Square::E1 && m.to == Square::G1),
        "the castle a player would drag should be in the list"
    );
    assert!(
        !moves
            .iter()
            .any(|m| m.from == Square::E1 && m.to == Square::H1),
        "and the library's spelling of it should not be"
    );
}

#[test]
fn san_names_the_moves_the_way_a_score_sheet_does() {
    let board = Board::from_fen(START, false).expect("fen");
    let e4 = to_engine(
        &board,
        PlayerMove {
            from: Square::E2,
            to: Square::E4,
            promotion: None,
        },
    );
    assert_eq!(san(&board, e4), "e4");

    let nf3 = to_engine(
        &board,
        PlayerMove {
            from: Square::G1,
            to: Square::F3,
            promotion: None,
        },
    );
    assert_eq!(san(&board, nf3), "Nf3");

    let castle = Board::from_fen(KIWIPETE, false).expect("fen");
    let oo = to_engine(
        &castle,
        PlayerMove {
            from: Square::E1,
            to: Square::G1,
            promotion: None,
        },
    );
    assert_eq!(san(&castle, oo), "O-O");
}

#[test]
fn san_disambiguates_when_two_pieces_can_reach_the_same_square() {
    // Two knights on d2 and f2 both reach e4. Printing "Ne4" for either is the
    // classic bug: it reads fine and round-trips to the wrong move.
    let board = Board::from_fen("4k3/8/8/8/8/8/3N1N2/4K3 w - - 0 1", false).expect("fen");
    let from_d2 = cozy_chess::Move {
        from: Square::D2,
        to: Square::E4,
        promotion: None,
    };
    let from_f2 = cozy_chess::Move {
        from: Square::F2,
        to: Square::E4,
        promotion: None,
    };
    assert_eq!(san(&board, from_d2), "Nde4");
    assert_eq!(san(&board, from_f2), "Nfe4");
}

#[test]
fn san_marks_check_and_mate() {
    // Back rank: Ra8 is mate, and the scenario says so.
    let board = Board::from_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", false).expect("fen");
    let mate = cozy_chess::Move {
        from: Square::A1,
        to: Square::A8,
        promotion: None,
    };
    assert_eq!(san(&board, mate), "Ra8#");
}

#[test]
fn a_pawn_capture_names_the_file_it_came_from() {
    let board = Board::from_fen("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1", false).expect("fen");
    let exd5 = cozy_chess::Move {
        from: Square::E4,
        to: Square::D5,
        promotion: None,
    };
    assert_eq!(san(&board, exd5), "exd5");
}

#[test]
fn every_scenario_is_a_legal_position_with_moves_to_make() {
    for scenario in strata_chess::scenarios::all() {
        let board = Board::from_fen(&scenario.fen, false)
            .unwrap_or_else(|e| panic!("{}: fen does not parse: {e:?}", scenario.id));
        let side = match board.side_to_move() {
            Color::White => "w",
            Color::Black => "b",
        };
        assert_eq!(
            side, scenario.you,
            "{}: the visitor should be the side to move",
            scenario.id
        );
        assert!(
            !legal_moves(&board).is_empty(),
            "{}: nothing to play",
            scenario.id
        );
        for field in [&scenario.title, &scenario.brief, &scenario.goal, &scenario.hint] {
            assert!(!field.trim().is_empty(), "{}: empty copy", scenario.id);
        }
    }
}

#[test]
fn the_levels_run_beginner_to_grandmaster_exactly_once() {
    let levels: Vec<String> = strata_chess::scenarios::all()
        .into_iter()
        .map(|s| s.level)
        .collect();
    assert_eq!(
        levels,
        vec!["beginner", "easy", "medium", "hard", "grandmaster"],
        "the ladder is the point; keep it in order and keep it complete"
    );
}

#[test]
fn the_stated_mates_really_are_mate_in_one() {
    // Two scenarios promise mate in one. Rather than trust the note, play the
    // move and ask the board.
    for (id, from, to) in [
        ("back-rank", Square::A1, Square::A8),
        ("smothered", Square::G5, Square::F7),
    ] {
        let scenario = strata_chess::scenarios::get(id).expect("scenario exists");
        let board = Board::from_fen(&scenario.fen, false).expect("fen");
        let mv = to_engine(&board, PlayerMove { from, to, promotion: None });
        let mut after = board.clone();
        after.play(mv);
        assert_eq!(
            after.status(),
            cozy_chess::GameStatus::Won,
            "{id}: the advertised move should be checkmate"
        );
    }
}

#[test]
fn promotion_is_offered_and_named() {
    let board = Board::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", false).expect("fen");
    let moves = legal_moves(&board);
    let promos: Vec<_> = moves
        .iter()
        .filter(|m| m.from == Square::A7 && m.to == Square::A8)
        .collect();
    assert_eq!(promos.len(), 4, "a pawn reaching the last rank has four choices");
    let queen = cozy_chess::Move {
        from: Square::A7,
        to: Square::A8,
        promotion: Some(Piece::Queen),
    };
    assert_eq!(san(&board, queen), "a8=Q+");
    let _ = (File::A, Rank::First);
}

#[test]
fn a_line_can_start_again_from_the_position_you_were_handed() {
    // Forking at ply 0 has no commit of its own to fork from: the first commit
    // is the first move. The point just before it has to work, because "try
    // that again from the top" is the commonest thing anyone does here.
    use strata_chess::store::{self, Ply};

    let mut db = store::open_cache().expect("cache");
    let ply = |san: &str| Ply {
        san: san.to_owned(),
        uci: "e2e4".to_owned(),
        fen: START.to_owned(),
        by: "w".to_owned(),
    };
    store::append(&mut db, store::MAIN, &ply("e4")).expect("first move");
    store::append(&mut db, store::MAIN, &ply("e5")).expect("second move");

    let timeline = store::timeline(&mut db, store::MAIN).expect("timeline");
    let before_first = timeline.first().expect("a commit").timestamp - 1;
    store::branch_at(&mut db, store::MAIN, "line-0", before_first).expect("fork before move one");

    assert!(
        store::line(&mut db, "line-0").expect("line").is_empty(),
        "a line started from the top has no moves on it"
    );
    assert_eq!(
        store::line(&mut db, store::MAIN).expect("line").len(),
        2,
        "and the line it came from still has both"
    );
}

#[test]
fn generated_candidates_are_legal_positions_or_nothing() {
    // The generator rejects seeds rather than repairing them, so the only
    // thing to prove is that what it does return is always playable: kings
    // apart, nobody already in check, pawns off the back ranks, moves to make.
    use cozy_chess::{Piece, Rank};
    let mut produced = 0;
    for level in ["beginner", "easy", "medium", "hard", "grandmaster"] {
        for seed in 0..400u32 {
            let Some(fen) = strata_chess::generate::candidate(level, seed) else {
                continue;
            };
            produced += 1;
            let board = Board::from_fen(&fen, false)
                .unwrap_or_else(|e| panic!("{level}/{seed}: {fen} does not parse: {e:?}"));
            assert!(
                board.checkers().is_empty(),
                "{level}/{seed}: {fen} hands over a position already in check"
            );
            assert!(
                !legal_moves(&board).is_empty(),
                "{level}/{seed}: {fen} has nothing to play"
            );
            for square in board.occupied() {
                if board.piece_on(square) == Some(Piece::Pawn) {
                    assert!(
                        square.rank() != Rank::First && square.rank() != Rank::Eighth,
                        "{level}/{seed}: {fen} has a pawn on the back rank"
                    );
                }
            }
        }
    }
    assert!(
        produced > 500,
        "the generator should find plenty of legal positions, found {produced}"
    );
}

#[test]
fn a_seed_always_makes_the_same_position() {
    // "Another one" counts seeds up. If a seed did not name a position exactly
    // there would be no way back to one someone liked.
    for level in ["beginner", "medium", "grandmaster"] {
        for seed in [1u32, 7, 99, 1234] {
            assert_eq!(
                strata_chess::generate::candidate(level, seed),
                strata_chess::generate::candidate(level, seed),
                "{level}/{seed} should be stable"
            );
        }
    }
}
