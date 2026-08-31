//! The acceptance criteria of issue #4, as far as they can be checked without a window:
//! a whole authoring session expressed as edits, the document it produces surviving a
//! save and a reopen, and undo unwinding the session step by step.

use campaign::document::Document;
use campaign::draft::{Draft, DraftShape};
use campaign::feature::{CellPoint, FeatureId, FeatureKind};
use campaign::gesture;
use campaign::world::World;

fn at(x: f32, y: f32) -> CellPoint {
    CellPoint::new(x, y)
}

fn author(document: &mut Document) -> (FeatureId, FeatureId, FeatureId, FeatureId) {
    let mut place = |kind, shape, vertices: &[CellPoint], label: &str| {
        let mut draft = Draft::new(kind, shape);
        for vertex in vertices {
            draft.push(*vertex);
        }
        let mut feature = draft.finish().expect("the draft must be finishable");
        feature.label = label.to_owned();
        let id = document.fresh_id();
        let edit = gesture::place(document.world(), id, feature);
        document.apply(edit).expect("the placement must apply");
        id
    };

    let road = place(
        FeatureKind::Road,
        DraftShape::Polyline,
        &[at(0.0, 40.0), at(30.0, 42.0), at(70.0, 45.0)],
        "the old south road",
    );
    let realm = place(
        FeatureKind::Territory,
        DraftShape::Polygon,
        &[at(-10.0, -10.0), at(200.0, -10.0), at(200.0, 200.0), at(-10.0, 200.0)],
        "the Vale",
    );
    let city = place(
        FeatureKind::Settlement,
        DraftShape::Polygon,
        &[at(20.0, 20.0), at(60.0, 20.0), at(60.0, 60.0), at(20.0, 60.0)],
        "Riverford",
    );
    let tavern = place(
        FeatureKind::Poi,
        DraftShape::Point,
        &[at(40.0, 40.0)],
        "the Eel",
    );

    (road, realm, city, tavern)
}

// The issue's first acceptance criterion, end to end: draw a road, a region, a city and a
// POI inside it, save, reopen, and get back exactly what was drawn — parent links, ids and
// the id counter included.
#[test]
fn a_drawn_session_survives_a_save_and_a_reopen_unchanged() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("world.ron");

    let mut document = Document::new(World::default());
    let (_road, _realm, city, tavern) = author(&mut document);

    assert_eq!(document.world().feature(tavern).unwrap().parent, Some(city));

    let before = document.world().clone();
    document.save(&path).expect("the session must be saveable");
    assert!(!document.is_dirty(), "a landed save clears the dirty flag");

    let reopened = Document::load(&path).expect("what was saved must reopen");
    assert_eq!(reopened.world(), &before, "the document must be identical");
}

// The second: undo unwinds a whole drawing session step by step, and redo replays it. The
// bound is real — UNDO_LIMIT entries — so a session longer than that is not fully
// reachable, and this pins the behaviour inside it.
#[test]
fn undo_unwinds_a_whole_session_and_redo_replays_it() {
    let mut document = Document::new(World::default());
    let empty = document.world().clone();
    author(&mut document);
    let authored = document.world().clone();

    assert_eq!(document.undo_depth(), 4, "one entry per placement");

    while document.undo().expect("every undo must apply") {}
    assert_eq!(
        document.world().features().count(),
        empty.features().count(),
        "undo must unwind the whole session"
    );

    while document.redo().expect("every redo must apply") {}
    assert_eq!(document.world(), &authored, "redo must replay it exactly");
}

// A save writes what a load reads: the round trip is byte-stable, so reopening twice in a
// row cannot drift.
#[test]
fn saving_a_reopened_session_produces_the_same_bytes() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let first = directory.path().join("world.ron");
    let second = directory.path().join("again.ron");

    let mut document = Document::new(World::default());
    author(&mut document);
    document.save(&first).expect("the first save must land");

    let mut reopened = Document::load(&first).expect("it must reopen");
    reopened.save(&second).expect("the second save must land");

    assert_eq!(
        std::fs::read_to_string(&first).unwrap(),
        std::fs::read_to_string(&second).unwrap()
    );
}
