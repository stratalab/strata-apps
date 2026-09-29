//! What the app promises, checked against the engine rather than a mock.
//!
//! The claim on the page is that the timeline is the database's history and a
//! scrub is a real read of the past. These tests are what makes that a claim
//! rather than a slogan: if as-of reads stopped working, or a stroke started
//! costing two commits, or history stopped reaching back to the first stroke,
//! one of them fails.

use strata_paint::store::{self, Commit};
use strata_paint::stroke::Stroke;

fn stroke(colour: &str, n: usize) -> Stroke {
    Stroke {
        colour: colour.to_owned(),
        width: 4.0,
        points: (0..n).map(|i| [i as f32 / 100.0, 0.5]).collect(),
        erase: false,
    }
}

#[test]
fn a_stroke_is_exactly_one_commit() {
    let mut db = store::open_cache().expect("cache opens");
    for i in 0..12 {
        store::append(&mut db, "default", &stroke("#fff", i + 2)).expect("stroke commits");
    }
    let timeline = store::timeline(&mut db, "default").expect("timeline reads");
    assert_eq!(
        timeline.len(),
        12,
        "twelve strokes should leave twelve commits, not one per key written"
    );
    assert_eq!(store::head(&mut db, "default").expect("head reads"), 12);
}

#[test]
fn the_timeline_runs_oldest_first_and_counts_up() {
    let mut db = store::open_cache().expect("cache opens");
    for i in 0..6 {
        store::append(&mut db, "default", &stroke("#fff", i + 2)).expect("stroke commits");
    }
    let timeline = store::timeline(&mut db, "default").expect("timeline reads");
    let counts: Vec<u64> = timeline.iter().map(|c| c.strokes).collect();
    assert_eq!(
        counts,
        vec![1, 2, 3, 4, 5, 6],
        "the slider reads left to right, so the timeline has to"
    );
    assert!(
        timeline.windows(2).all(|w| w[0].timestamp < w[1].timestamp),
        "commit timestamps must increase along the timeline"
    );
}

#[test]
fn scrubbing_returns_the_painting_as_it_was() {
    let mut db = store::open_cache().expect("cache opens");
    let mut commits: Vec<Commit> = Vec::new();
    for i in 0..10 {
        commits.push(store::append(&mut db, "default", &stroke("#fff", i + 2)).expect("stroke commits"));
    }

    // Every point on the timeline holds exactly the strokes drawn up to it.
    for (i, commit) in commits.iter().enumerate() {
        let past = store::canvas_at(&mut db, "default", commit.timestamp).expect("as-of read");
        assert_eq!(
            past.len(),
            i + 1,
            "at commit {} the painting should hold {} stroke(s)",
            commit.timestamp,
            i + 1
        );
    }

    // And the present is still the present: reading the past does not move it.
    assert_eq!(store::canvas_now(&mut db, "default").expect("canvas reads").len(), 10);
}

#[test]
fn history_reaches_the_first_stroke_after_many() {
    // The retained window is the thing that would quietly break a scrubber:
    // it works in testing with ten strokes and fails on a real drawing. Three
    // hundred is past where a demo would ever go.
    let mut db = store::open_cache().expect("cache opens");
    let first = store::append(&mut db, "default", &stroke("#f00", 3)).expect("stroke commits");
    for _ in 0..300 {
        store::append(&mut db, "default", &stroke("#fff", 3)).expect("stroke commits");
    }

    let past = store::canvas_at(&mut db, "default", first.timestamp).expect("as-of read at the first stroke");
    assert_eq!(past.len(), 1, "the first commit still holds one stroke");
    assert_eq!(
        past[0].colour, "#f00",
        "and it is the stroke that was actually drawn first"
    );
    assert_eq!(store::timeline(&mut db, "default").expect("timeline").len(), 301);
}

#[test]
fn a_scrub_reads_the_strokes_that_existed_not_the_ones_that_do() {
    // The bug this guards: reading the keys as-of but the values at latest is
    // only correct because a stroke never changes. If a stroke ever becomes
    // mutable, this is the test that should stop it silently rewriting history.
    let mut db = store::open_cache().expect("cache opens");
    let red = store::append(&mut db, "default", &stroke("#f00", 3)).expect("stroke commits");
    store::append(&mut db, "default", &stroke("#00f", 3)).expect("stroke commits");

    let past = store::canvas_at(&mut db, "default", red.timestamp).expect("as-of read");
    assert_eq!(past.len(), 1);
    assert_eq!(past[0].colour, "#f00");
}

#[test]
fn an_empty_painting_has_no_timeline() {
    let mut db = store::open_cache().expect("cache opens");
    assert!(store::timeline(&mut db, "default").expect("timeline reads").is_empty());
    assert_eq!(store::head(&mut db, "default").expect("head reads"), 0);
    assert!(store::canvas_now(&mut db, "default").expect("canvas reads").is_empty());
}

#[test]
fn a_stroke_that_could_not_be_drawn_is_refused() {
    let mut db = store::open_cache().expect("cache opens");
    let mut empty = stroke("#fff", 0);
    empty.points.clear();
    assert!(store::append(&mut db, "default", &empty).is_err(), "no points");

    let mut wide = stroke("#fff", 3);
    wide.width = 0.0;
    assert!(store::append(&mut db, "default", &wide).is_err(), "zero width");

    let mut nan = stroke("#fff", 3);
    nan.points[1][0] = f32::NAN;
    assert!(store::append(&mut db, "default", &nan).is_err(), "not finite");

    // A refused stroke leaves no commit behind.
    assert!(store::timeline(&mut db, "default").expect("timeline reads").is_empty());
}
