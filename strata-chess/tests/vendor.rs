//! The licence notice has to describe the binary that is actually there.
//!
//! `vendor/stockfish/README.md` states a version, says the files are unmodified
//! and gives their hashes. That is a licence claim, not a comment: GPLv3 asks
//! for the Corresponding Source of *these* binaries. If someone rebuilds or
//! patches Stockfish and the notice still says "copied byte for byte", the
//! notice is wrong in a way that matters. This makes that a failing test rather
//! than a thing nobody notices.

use std::path::Path;

fn sha256(path: &str) -> String {
    use std::process::Command;
    let out = Command::new("sha256sum")
        .arg(path)
        .output()
        .expect("sha256sum runs");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

const JS: &str = "d3344124ab067fb0b90ee77873bb8e9fbf5fc01bc525fe714b0f942581e889e6";
const WASM: &str = "57ac2d72312aba346760e3f173f687a8c211208e97a87268436f7f0e10bb5387";

#[test]
fn the_vendored_stockfish_is_the_one_the_notice_describes() {
    let js = "vendor/stockfish/stockfish-19-lite-single.js";
    let wasm = "vendor/stockfish/stockfish-19-lite-single.wasm";
    assert!(Path::new(js).exists(), "{js} is missing");
    assert!(Path::new(wasm).exists(), "{wasm} is missing");
    assert_eq!(sha256(js), JS, "{js} changed; update vendor/stockfish/README.md");
    assert_eq!(
        sha256(wasm),
        WASM,
        "{wasm} changed; update vendor/stockfish/README.md"
    );
}

#[test]
fn the_licence_travels_with_it() {
    let copying = std::fs::read_to_string("vendor/stockfish/Copying.txt").expect("Copying.txt");
    assert!(
        copying.contains("GNU GENERAL PUBLIC LICENSE") && copying.contains("Version 3"),
        "Copying.txt should be the GPL-3.0 text"
    );
    let notice = std::fs::read_to_string("vendor/stockfish/README.md").expect("README.md");
    for expected in [
        "GPL-3.0",
        "Corresponding Source",
        "official-stockfish/Stockfish",
        &JS[..16],
        &WASM[..16],
    ] {
        assert!(notice.contains(expected), "the notice should mention {expected}");
    }
}
