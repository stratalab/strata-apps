//! Can you rewind and branch from there?
//!
//! Yes: `fork_at_timestamp` and `fork_at_version` take a retained point on the
//! source branch's timeline and start a new branch from it, rather than from
//! the head. These tests pin that against the real engine, in this app's own
//! data model, so the answer stays true rather than being a note about an API
//! that once existed.

use stratadb::branch::BranchStateSelector;
use stratadb::kv::KvKey;
use stratadb::{BranchName, Database, ProductSpace};

use strata_paint::store::{self, STROKE_PREFIX};
use strata_paint::stroke::Stroke;

fn stroke(colour: &str) -> Stroke {
    Stroke {
        colour: colour.to_owned(),
        width: 6.0,
        points: vec![[0.1, 0.1], [0.5, 0.5], [0.9, 0.2]],
        erase: false,
    }
}

fn count_strokes(db: &mut Database, branch: &str) -> usize {
    let prefix = KvKey::new(STROKE_PREFIX.as_bytes().to_vec()).expect("prefix");
    db.kv(
        BranchName::new(branch).expect("branch"),
        ProductSpace::new(store::SPACE).expect("space"),
    )
    .expect("kv")
    .list(Some(&prefix))
    .expect("list")
    .len()
}

#[test]
fn a_branch_can_start_from_a_past_commit() {
    let mut db = store::open_cache().expect("cache opens");

    // Six strokes on the one timeline.
    let mut commits = Vec::new();
    for colour in ["#f00", "#0f0", "#00f", "#ff0", "#0ff", "#f0f"] {
        commits.push(store::append(&mut db, "default", &stroke(colour)).expect("stroke commits"));
    }
    assert_eq!(count_strokes(&mut db, "default"), 6);

    // Rewind to the third stroke and branch from there.
    let rewind = commits[2];
    db.branches()
        .expect("branches")
        .fork_at_timestamp(
            &BranchName::new("default").expect("source"),
            BranchName::new("what-if").expect("name"),
            stratadb::Timestamp::from_micros(rewind.timestamp),
        )
        .expect("fork at a past timestamp");

    // The fork holds the painting as it was, not as it is.
    assert_eq!(
        count_strokes(&mut db, "what-if"),
        3,
        "a branch forked at the third commit should hold three strokes"
    );
    assert_eq!(
        count_strokes(&mut db, "default"),
        6,
        "and the branch it was taken from is untouched"
    );
}

#[test]
fn the_two_timelines_then_go_their_own_ways() {
    let mut db = store::open_cache().expect("cache opens");
    let mut commits = Vec::new();
    for colour in ["#f00", "#0f0", "#00f", "#ff0"] {
        commits.push(store::append(&mut db, "default", &stroke(colour)).expect("stroke commits"));
    }

    db.branches()
        .expect("branches")
        .fork_at_timestamp(
            &BranchName::new("default").expect("source"),
            BranchName::new("what-if").expect("name"),
            stratadb::Timestamp::from_micros(commits[1].timestamp),
        )
        .expect("fork at a past timestamp");

    // Paint on the fork. `append` writes to the default branch, so this goes
    // through the same keys by hand to keep the test about branching.
    let prefix = ProductSpace::new(store::SPACE).expect("space");
    let mut kv = db
        .kv(BranchName::new("what-if").expect("branch"), prefix)
        .expect("kv");
    kv.put(
        KvKey::new(format!("{STROKE_PREFIX}00000003").into_bytes()).expect("key"),
        stratadb::kv::KvValue::new(serde_json::to_vec(&stroke("#fff")).expect("encodes")),
    )
    .expect("write on the fork");
    drop(kv);

    assert_eq!(count_strokes(&mut db, "what-if"), 3);
    assert_eq!(
        count_strokes(&mut db, "default"),
        4,
        "the original timeline kept its own fourth stroke"
    );

    // And the engine can say how they differ.
    let diff = db
        .branches()
        .expect("branches")
        .compare(
            &BranchName::new("default").expect("a"),
            &BranchName::new("what-if").expect("b"),
            BranchStateSelector::Current,
        )
        .expect("compare");
    assert!(
        !diff.comparisons().is_empty(),
        "two timelines that diverged should compare as different"
    );
    assert_eq!(diff.branch_a().as_str(), "default");
    assert_eq!(diff.branch_b().as_str(), "what-if");
}

#[test]
fn forking_at_a_version_works_the_same_way() {
    let mut db = store::open_cache().expect("cache opens");
    let mut commits = Vec::new();
    for colour in ["#f00", "#0f0", "#00f", "#ff0", "#0ff"] {
        commits.push(store::append(&mut db, "default", &stroke(colour)).expect("stroke commits"));
    }

    db.branches()
        .expect("branches")
        .fork_at_version(
            &BranchName::new("default").expect("source"),
            BranchName::new("by-version").expect("name"),
            stratadb::CommitVersion::new(commits[1].version),
        )
        .expect("fork at a past version");

    assert_eq!(count_strokes(&mut db, "by-version"), 2);
}

#[test]
fn a_fork_carries_the_history_it_was_taken_from() {
    // What the scrubber on a new branch shows depends entirely on this. If a
    // fork started with an empty history the slider would open with nothing in
    // it, and the strokes on the canvas would have no commits behind them.
    let mut db = store::open_cache().expect("cache opens");
    let mut commits = Vec::new();
    for colour in ["#f00", "#0f0", "#00f", "#ff0", "#0ff"] {
        commits.push(store::append(&mut db, "default", &stroke(colour)).expect("commits"));
    }

    store::fork_at(&mut db, "default", "what-if", commits[2].timestamp).expect("fork");

    let forked = store::timeline(&mut db, "what-if").expect("fork timeline");
    let origin = store::timeline(&mut db, "default").expect("origin timeline");
    eprintln!(
        "fork timeline: {} commits (counts {:?}); origin: {} commits",
        forked.len(),
        forked.iter().map(|c| c.strokes).collect::<Vec<_>>(),
        origin.len()
    );

    assert_eq!(
        store::canvas_now(&mut db, "what-if").expect("canvas").len(),
        3,
        "the fork opens on the painting as it was at the third commit"
    );

    // Painting on the fork extends the fork, not the branch it came from.
    store::append(&mut db, "what-if", &stroke("#fff")).expect("commits on the fork");
    assert_eq!(store::head(&mut db, "what-if").expect("head"), 4);
    assert_eq!(store::head(&mut db, "default").expect("head"), 5);
    assert_eq!(store::branch_names(&mut db).expect("names"), vec!["default", "what-if"]);
}
