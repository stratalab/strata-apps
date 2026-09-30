//! The browser build.
//!
//! JavaScript owns the board, the clock and the Stockfish worker; this owns the
//! rules and the database. Anything that decides whether a move is legal, or
//! what the line looks like, happens here, because those are the two things a
//! UI should never be trusted with.

use cozy_chess::{Board, Color, GameStatus, Piece, Square};
use serde_json::json;
use stratadb::Database;
use wasm_bindgen::prelude::*;

use crate::game::{self, PlayerMove};
use crate::scenarios;
use crate::store::{self, Ply};

#[wasm_bindgen]
pub struct Chess {
    db: Database,
    board: Board,
    /// The line being played.
    on: String,
    /// Where this line started, so the board can be rebuilt from ply 0.
    start_fen: String,
    scenario: String,
}

fn fail(message: &str) -> JsValue {
    JsValue::from_str(message)
}

fn square(name: &str) -> Result<Square, JsValue> {
    name.parse::<Square>()
        .map_err(|_| fail(&format!("not a square: {name}")))
}

fn promotion(name: &str) -> Option<Piece> {
    match name {
        "q" => Some(Piece::Queen),
        "r" => Some(Piece::Rook),
        "b" => Some(Piece::Bishop),
        "n" => Some(Piece::Knight),
        _ => None,
    }
}

fn status_word(board: &Board) -> &'static str {
    match board.status() {
        GameStatus::Won => "checkmate",
        GameStatus::Drawn => "draw",
        GameStatus::Ongoing => {
            if board.checkers().is_empty() {
                "ongoing"
            } else {
                "check"
            }
        }
    }
}

#[wasm_bindgen]
impl Chess {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Chess, JsValue> {
        let db = store::open_cache().map_err(|e| fail(&e.to_string()))?;
        Ok(Chess {
            db,
            board: Board::default(),
            on: store::MAIN.to_owned(),
            start_fen: Board::default().to_string(),
            scenario: String::new(),
        })
    }

    /// The scenario list, for the picker.
    pub fn scenarios(&self) -> Result<String, JsValue> {
        serde_json::to_string(&scenarios::all()).map_err(|e| fail(&e.to_string()))
    }

    /// A legal position at this level for this seed, or "" if the seed did not
    /// make one. The caller counts seeds up; nothing loops here.
    pub fn candidate(&self, level: &str, seed: u32) -> String {
        crate::generate::candidate(level, seed).unwrap_or_default()
    }

    /// Starts a scenario handed in from outside: a generated one.
    pub fn load_fen(&mut self, id: &str, fen: &str) -> Result<String, JsValue> {
        let board = Board::from_fen(fen, false)
            .map_err(|e| fail(&format!("generated position is not legal: {e:?}")))?;
        self.db = store::open_cache().map_err(|e| fail(&e.to_string()))?;
        self.board = board;
        self.on = store::MAIN.to_owned();
        self.start_fen = fen.to_owned();
        self.scenario = id.to_owned();
        self.state()
    }

    /// Starts a scenario on a fresh database, so each one gets its own tree.
    pub fn load(&mut self, id: &str) -> Result<String, JsValue> {
        let scenario = scenarios::get(id).ok_or_else(|| fail(&format!("no scenario {id}")))?;
        let board = Board::from_fen(&scenario.fen, false)
            .map_err(|e| fail(&format!("scenario {id} has a bad fen: {e:?}")))?;
        self.db = store::open_cache().map_err(|e| fail(&e.to_string()))?;
        self.board = board;
        self.on = store::MAIN.to_owned();
        self.start_fen = scenario.fen.clone();
        self.scenario = id.to_owned();
        self.state()
    }

    /// Everything the UI draws from.
    pub fn state(&mut self) -> Result<String, JsValue> {
        let moves: Vec<_> = game::legal_moves(&self.board)
            .into_iter()
            .map(|m| {
                json!({
                    "from": m.from.to_string(),
                    "to": m.to.to_string(),
                    "promotion": m.promotion.map(|p| p.to_string()),
                })
            })
            .collect();
        let line = store::line(&mut self.db, &self.on).map_err(|e| fail(&e.to_string()))?;
        let lines = store::lines(&mut self.db).map_err(|e| fail(&e.to_string()))?;
        let timeline = store::timeline(&mut self.db, &self.on).map_err(|e| fail(&e.to_string()))?;

        Ok(json!({
            "scenario": self.scenario,
            "fen": self.board.to_string(),
            "startFen": self.start_fen,
            "turn": match self.board.side_to_move() { Color::White => "w", Color::Black => "b" },
            "status": status_word(&self.board),
            "legal": moves,
            "line": line,
            "lines": lines,
            "on": self.on,
            "timeline": store::commits_json(&timeline),
        })
        .to_string())
    }

    /// Plays one half-move, if it is legal. Returns the new state.
    ///
    /// Legality is decided here and only here. The UI offers what `state`
    /// listed, but a UI can be wrong or impatient, so this checks again.
    pub fn play(&mut self, from: &str, to: &str, promo: &str) -> Result<String, JsValue> {
        let wanted = PlayerMove {
            from: square(from)?,
            to: square(to)?,
            promotion: promotion(promo),
        };
        // The promotion is a preference, not part of the move's identity. A UI
        // that always sends "q" is being helpful, and asking for a queen on a
        // rook move should not make the rook move illegal.
        let legal = game::legal_moves(&self.board);
        let candidates: Vec<PlayerMove> = legal
            .into_iter()
            .filter(|m| m.from == wanted.from && m.to == wanted.to)
            .collect();
        let matched = candidates
            .iter()
            .find(|m| m.promotion == wanted.promotion)
            .or_else(|| candidates.first())
            .copied()
            .ok_or_else(|| fail(&format!("illegal move {from}{to}")))?;

        let engine_move = game::to_engine(&self.board, matched);
        let san = game::san(&self.board, engine_move);
        let by = match self.board.side_to_move() {
            Color::White => "w",
            Color::Black => "b",
        };
        self.board.play(engine_move);

        let ply = Ply {
            san,
            uci: format!(
                "{}{}{}",
                matched.from,
                matched.to,
                matched.promotion.map(|p| p.to_string()).unwrap_or_default()
            ),
            fen: self.board.to_string(),
            by: by.to_owned(),
        };
        store::append(&mut self.db, &self.on.clone(), &ply).map_err(|e| fail(&e))?;
        self.state()
    }

    /// Goes back to the position after `ply` half-moves on this line, without
    /// forking. Nothing is lost: the line still holds everything it held.
    pub fn rewind(&mut self, ply: usize) -> Result<String, JsValue> {
        let line = store::line(&mut self.db, &self.on).map_err(|e| fail(&e.to_string()))?;
        let fen = if ply == 0 {
            self.start_fen.clone()
        } else {
            line.get(ply - 1)
                .map(|p| p.fen.clone())
                .ok_or_else(|| fail("no such ply on this line"))?
        };
        self.board = Board::from_fen(&fen, false).map_err(|e| fail(&format!("{e:?}")))?;
        self.state()
    }

    /// Starts a variation from the position after `ply` half-moves.
    ///
    /// This is the move that was not played. The line it came from keeps
    /// everything it had, which is the whole reason a wrong try costs nothing.
    pub fn branch(&mut self, ply: u32) -> Result<String, JsValue> {
        let timeline = store::timeline(&mut self.db, &self.on).map_err(|e| fail(&e.to_string()))?;
        let name = store::free_name(&mut self.db, ply as u64).map_err(|e| fail(&e.to_string()))?;

        let first = timeline
            .first()
            .ok_or_else(|| fail("nothing has been played on this line yet"))?;

        // Starting again from the position you were handed is the commonest
        // retry there is, and there is no commit at ply 0 to fork from: the
        // first commit is the first move. The point just before it is a real,
        // retained place on the timeline where the line was still empty, so
        // that is where a fresh attempt begins.
        let at = if ply == 0 {
            first.timestamp.saturating_sub(1)
        } else {
            timeline
                .get(ply as usize - 1)
                .ok_or_else(|| fail("no commit at that ply"))?
                .timestamp
        };
        let source = self.on.clone();
        store::branch_at(&mut self.db, &source, &name, at).map_err(|e| fail(&e))?;
        self.on = name;
        self.rewind(ply as usize)
    }

    /// Switches to another line, at its latest position.
    pub fn use_line(&mut self, name: &str) -> Result<String, JsValue> {
        let names = store::lines(&mut self.db).map_err(|e| fail(&e.to_string()))?;
        if !names.iter().any(|n| n == name) {
            return Err(fail(&format!("no line called {name}")));
        }
        self.on = name.to_owned();
        let line = store::line(&mut self.db, &self.on).map_err(|e| fail(&e.to_string()))?;
        let fen = line
            .last()
            .map(|p| p.fen.clone())
            .unwrap_or_else(|| self.start_fen.clone());
        self.board = Board::from_fen(&fen, false).map_err(|e| fail(&format!("{e:?}")))?;
        self.state()
    }
}
