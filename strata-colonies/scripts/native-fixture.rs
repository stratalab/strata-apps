// Compiled directly by the browser verifier: no duplicate reference algorithm.
#[allow(dead_code)]
#[path = "../src/life.rs"]
mod life;
#[allow(dead_code)]
#[path = "../src/patterns.rs"]
mod patterns;

fn main() {
    for (width, height, seed) in [(64, 48, 42), (16, 12, 7), (9, 11, 19)] {
        let initial = patterns::genesis(width, height, seed);
        let mut boards = vec![initial.clone()];
        let mut used = std::collections::HashSet::new();
        for index in 1..=5 {
            let cell = patterns::perturbation(&initial, index, &used);
            used.insert(cell);
            let mut board = initial.clone();
            board.flip(cell.0, cell.1);
            boards.push(board);
        }
        for generation in 0..=20 {
            for (index, board) in boards.iter().enumerate() {
                let hex: String = board.packed().iter().map(|b| format!("{b:02x}")).collect();
                println!("{width},{height},{seed},{index},{generation},{hex}");
            }
            boards = boards.iter().map(life::Board::step).collect();
        }
    }
}
