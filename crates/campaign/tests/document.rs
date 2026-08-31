//! What a document remembers: how far back undo reaches, what redo does after it, and
//! when the GM should be asked before closing.

use campaign::document::{Document, UNDO_LIMIT};
use campaign::edit::Edit;
use campaign::feature::{CellPoint, Feature, FeatureId, FeatureKind, Geometry};
use campaign::world::World;

fn point(x: f32, y: f32) -> Geometry {
    Geometry::Point(CellPoint::new(x, y))
}

fn feature(kind: FeatureKind, geometry: Geometry, label: &str) -> Feature {
    Feature {
        kind,
        geometry,
        label: label.to_owned(),
        note: None,
        parent: None,
    }
}

fn bytes(document: &Document) -> String {
    document.world().to_ron().expect("serialize")
}

fn fixture() -> (Document, Vec<FeatureId>) {
    let mut document = Document::new(World::default());
    let mut ids = Vec::new();

    for candidate in [
        feature(FeatureKind::Poi, point(3.5, -2.25), "a standing stone"),
        feature(
            FeatureKind::Road,
            Geometry::Polyline(vec![
                CellPoint::new(0.0, 0.0),
                CellPoint::new(10.0, 0.5),
                CellPoint::new(20.0, 3.0),
            ]),
            "the old south road",
        ),
        feature(
            FeatureKind::Settlement,
            Geometry::Polygon(vec![
                CellPoint::new(30.0, 30.0),
                CellPoint::new(40.0, 30.0),
                CellPoint::new(35.0, 40.0),
            ]),
            "Riverford",
        ),
    ] {
        let id = document.fresh_id();
        document
            .apply(Edit::Add {
                id,
                feature: candidate,
            })
            .expect("the fixture must apply");
        ids.push(id);
    }

    let mut inn = feature(FeatureKind::Poi, point(34.0, 33.0), "the Eel");
    inn.parent = Some(ids[2]);
    let id = document.fresh_id();
    document
        .apply(Edit::Add { id, feature: inn })
        .expect("the tavern");
    ids.push(id);

    (document, ids)
}

// Acceptance criterion 4: undo across a sequence of mixed edits restores the original.
#[test]
fn undoing_a_run_of_mixed_edits_restores_the_document() {
    let (mut document, ids) = fixture();
    let [poi, road, city, tavern] = [ids[0], ids[1], ids[2], ids[3]];
    let original = bytes(&document);

    let spare = document.fresh_id();
    let after_allocation = bytes(&document);

    let edits = vec![
        Edit::SetLabel {
            id: city,
            label: "Riverford-on-Eel".to_owned(),
        },
        Edit::MoveVertex {
            id: road,
            index: 1,
            to: CellPoint::new(11.0, 1.5),
        },
        Edit::InsertVertex {
            id: road,
            index: 3,
            at: CellPoint::new(30.0, 6.0),
        },
        Edit::SetParent {
            id: poi,
            parent: Some(city),
        },
        Edit::Add {
            id: spare,
            feature: feature(FeatureKind::River, point(1.0, 2.0), "the Eel Water"),
        },
        Edit::SetKind {
            id: poi,
            kind: FeatureKind::DungeonEntry,
        },
        Edit::Delete { id: tavern },
        Edit::SetNote {
            id: city,
            note: Some("places/riverford.md".to_owned()),
        },
        Edit::RemoveVertex {
            id: road,
            index: 0,
        },
    ];
    let before_edits = document.undo_depth();
    let applied = edits.len();
    for edit in edits {
        document.apply(edit).expect("each edit applies");
    }

    assert_eq!(document.undo_depth(), before_edits + applied);
    for step in 0..applied {
        assert!(
            document.undo().expect("undo"),
            "step {step} of {applied} had nothing to undo"
        );
    }

    assert_eq!(
        bytes(&document),
        after_allocation,
        "undoing everything must restore the document the edits started from"
    );
    assert_ne!(
        bytes(&document),
        original,
        "the id handed out along the way is spent, and undo does not reclaim it"
    );
}

// Undo must not run through the path that clears the redo stack, or it would discard the
// entry it has just pushed and silently do nothing the second time round.
#[test]
fn undo_and_redo_cycle_rather_than_cancelling_each_other() {
    let (mut document, ids) = fixture();
    let before = bytes(&document);

    document
        .apply(Edit::SetLabel {
            id: ids[2],
            label: "Riverford-on-Eel".to_owned(),
        })
        .expect("apply");
    let after = bytes(&document);

    assert!(document.undo().expect("undo"));
    assert_eq!(bytes(&document), before);
    assert_eq!(document.redo_depth(), 1);

    assert!(document.redo().expect("redo"));
    assert_eq!(bytes(&document), after);

    assert!(document.undo().expect("undo again"));
    assert_eq!(bytes(&document), before);
}

// A new change makes the undone future unreachable, which is what everyone expects of an
// undo stack and what stops redo replaying edits against a world that moved on.
#[test]
fn applying_an_edit_clears_the_redo_stack() {
    let (mut document, ids) = fixture();

    document
        .apply(Edit::SetLabel {
            id: ids[2],
            label: "first".to_owned(),
        })
        .expect("apply");
    document.undo().expect("undo");
    assert_eq!(document.redo_depth(), 1);

    document
        .apply(Edit::SetLabel {
            id: ids[2],
            label: "second".to_owned(),
        })
        .expect("apply");

    assert_eq!(document.redo_depth(), 0);
}

// Nothing to undo is an ordinary answer, not a failure — an editor should not have to
// track whether the stack is empty to avoid an error.
#[test]
fn undo_and_redo_on_an_empty_stack_do_nothing() {
    let (mut document, _) = fixture();
    while document.undo().expect("drain") {}
    let drained = bytes(&document);

    assert!(!document.undo().expect("nothing left"));
    assert_eq!(bytes(&document), drained);

    while document.redo().expect("rewind") {}
    assert!(!document.redo().expect("nothing left"));
}

// The stack is bounded, and reaching the bound drops the oldest rather than refusing the
// newest: a GM mid-stroke is never told their history is full.
#[test]
fn the_undo_stack_is_bounded_and_drops_its_oldest_entry() {
    let (mut document, ids) = fixture();
    let road = ids[1];

    document
        .apply(Edit::SetLabel {
            id: road,
            label: "the first change".to_owned(),
        })
        .expect("apply");
    let after_first = bytes(&document);

    for step in 0..UNDO_LIMIT {
        document
            .apply(Edit::SetLabel {
                id: road,
                label: format!("change {step}"),
            })
            .expect("apply");
    }

    assert_eq!(document.undo_depth(), UNDO_LIMIT);
    while document.undo().expect("undo") {}

    assert_eq!(
        bytes(&document),
        after_first,
        "undoing every retained inverse reaches the state just before the oldest of them"
    );
}

// A batch is one entry however many edits it carries, which is what makes a cascading
// delete a single press of undo.
#[test]
fn a_batch_is_one_step_on_the_undo_stack() {
    let (mut document, ids) = fixture();
    let before = bytes(&document);

    let before_batch = document.undo_depth();
    document
        .apply(Edit::Batch(vec![
            Edit::Delete { id: ids[3] },
            Edit::Delete { id: ids[2] },
        ]))
        .expect("children first, then the parent");
    assert_eq!(
        document.undo_depth(),
        before_batch + 1,
        "two deletes went on as one entry"
    );
    assert!(document.world().feature(ids[2]).is_none());

    assert!(document.undo().expect("undo"));

    assert_eq!(bytes(&document), before, "one press brought the city back");
}

// The flag exists to decide whether to prompt on close, so it has to be true whenever
// what is in memory differs from what was last written — undo included.
#[test]
fn dirty_tracks_whether_anything_differs_from_the_last_save() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("world.ron");
    let (mut document, ids) = fixture();
    assert!(document.is_dirty(), "the fixture applied edits");

    document.save(&path).expect("save");
    assert!(!document.is_dirty());

    document
        .apply(Edit::SetLabel {
            id: ids[2],
            label: "Riverford-on-Eel".to_owned(),
        })
        .expect("apply");
    assert!(document.is_dirty());

    document.save(&path).expect("save");
    assert!(!document.is_dirty());

    document.undo().expect("undo");
    assert!(
        document.is_dirty(),
        "what is on disk is what was written, not wherever the stack has been wound to"
    );
}

// A refused edit is not a change, so it must not cost the GM a prompt on close or an
// entry on either stack.
#[test]
fn a_refused_edit_touches_neither_stack_nor_the_dirty_flag() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("world.ron");
    let (mut document, ids) = fixture();
    document.save(&path).expect("save");
    let depth = document.undo_depth();

    document
        .apply(Edit::Delete { id: ids[2] })
        .expect_err("the city still holds the tavern");

    assert!(!document.is_dirty());
    assert_eq!(document.undo_depth(), depth);
}

// Saving is not a change: it must not cost the GM the history they could still undo.
#[test]
fn saving_keeps_the_undo_stack() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("world.ron");
    let (mut document, _) = fixture();
    let depth = document.undo_depth();

    document.save(&path).expect("save");

    assert_eq!(document.undo_depth(), depth);
    assert!(document.undo().expect("undo still reaches back past the save"));
}

// A document written and read back is the same document, which is what makes the undo
// stack the only thing a session loses.
#[test]
fn a_document_round_trips_through_its_file() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("world.ron");
    let (mut document, _) = fixture();

    document.save(&path).expect("save");
    let read = Document::load(&path).expect("load");

    assert_eq!(read.world(), document.world());
    assert!(!read.is_dirty());
    assert_eq!(read.undo_depth(), 0);
}
