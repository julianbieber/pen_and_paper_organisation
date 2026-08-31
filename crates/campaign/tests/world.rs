//! What a world document admits and refuses: what survives a round trip through
//! `world.ron`, and every shape of incoherent document that must be reported rather than
//! loaded.

use std::path::Path;

use campaign::edit::Edit;
use campaign::feature::{CellPoint, Feature, FeatureId, FeatureKind, Geometry};
use campaign::world::{ParentProblem, World, WorldError};

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

fn add(world: &mut World, feature: Feature) -> FeatureId {
    let id = world.fresh_id();
    Edit::Add { id, feature }
        .apply(world)
        .expect("the fixture must be addable, or it tests nothing");
    id
}

fn populated() -> (World, Vec<FeatureId>) {
    let mut world = World::default();
    let mut ids = Vec::new();

    ids.push(add(
        &mut world,
        feature(FeatureKind::Poi, point(3.5, -2.25), "a standing stone"),
    ));
    ids.push(add(
        &mut world,
        feature(
            FeatureKind::Road,
            Geometry::Polyline(vec![
                CellPoint::new(0.0, 0.0),
                CellPoint::new(10.0, 0.5),
                CellPoint::new(20.0, 3.0),
            ]),
            "the old south road",
        ),
    ));

    let city = add(
        &mut world,
        feature(
            FeatureKind::Settlement,
            Geometry::Polygon(vec![
                CellPoint::new(30.0, 30.0),
                CellPoint::new(40.0, 30.0),
                CellPoint::new(35.0, 40.0),
            ]),
            "Riverford",
        ),
    );
    ids.push(city);

    let mut tavern = feature(FeatureKind::Poi, point(34.0, 33.0), "the Eel");
    tavern.parent = Some(city);
    tavern.note = Some("places/the-eel.md".to_owned());
    ids.push(add(&mut world, tavern));

    (world, ids)
}

fn nowhere() -> &'static Path {
    Path::new("world.ron")
}

// Acceptance criterion 2: a document survives the file unchanged, parent links included.
#[test]
fn a_world_round_trips_through_world_ron() {
    let (world, _) = populated();
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("world.ron");

    world.save(&path).expect("save");
    let read = World::load(&path).expect("load");

    assert_eq!(read, world);
    let child = read
        .features()
        .find(|(_, feature)| feature.label == "the Eel")
        .expect("the tavern survived");
    assert!(child.1.parent.is_some(), "the parent link survived");
    assert_eq!(child.1.note.as_deref(), Some("places/the-eel.md"));
}

// The identical-saves invariant is about bytes, which a round trip alone cannot show:
// an unordered map round-trips fine and still writes a different file each time.
#[test]
fn two_saves_of_one_world_are_byte_identical() {
    let (world, _) = populated();

    let first = world.to_ron().expect("serialize");
    let second = world.to_ron().expect("serialize");

    assert_eq!(first, second);
    assert!(
        first.find("Riverford").unwrap() < first.find("the Eel").unwrap(),
        "features are written in id order, which is what makes the bytes stable"
    );
}

// An absent document is an empty world: `Campaign::create` writes none, so "missing"
// must read as "default" rather than needing a migration.
#[test]
fn an_absent_world_document_is_an_empty_world() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("world.ron");

    let world = World::load(&path).expect("an absent document loads");

    assert!(world.is_empty());
    assert!(!path.exists(), "loading must not create the file");
}

// An empty world is a real document, not a special case that only exists in memory.
#[test]
fn an_empty_world_round_trips() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("world.ron");
    let world = World::default();

    world.save(&path).expect("save");

    assert_eq!(World::load(&path).expect("load"), world);
}

// Something at the path that is not a regular file is refused, never read as absent —
// reading a FIFO as an empty world would block forever instead of reporting anything.
#[test]
fn a_path_that_is_not_a_regular_file_is_refused() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("world.ron");
    std::fs::create_dir(&path).expect("a directory where the document should be");

    let error = World::load(&path).expect_err("a directory is not a document");

    assert!(matches!(error, WorldError::WorldUnreadable { .. }));
}

// The format carries a version and refuses any other, for the reason the manifest does:
// there is no migration path, so a later format is reported rather than half-read.
#[test]
fn a_world_from_another_format_version_is_refused() {
    let error = World::from_ron("(version: 99, next_id: 0, features: {})", nowhere())
        .expect_err("a version this build does not read");

    assert!(matches!(
        error,
        WorldError::UnsupportedVersion { found: 99, .. }
    ));
}

// A later build's additional field must not make a document unreadable here: the format
// has to stay open enough to gain a per-feature flag without breaking saves.
#[test]
fn an_unknown_field_is_ignored_rather_than_refused() {
    let text = "(version: 1, next_id: 0, features: {}, revealed_by_default: true)";

    let world = World::from_ron(text, nowhere()).expect("an unknown field is ignored");

    assert!(world.is_empty());
}

// Acceptance criterion 3: a dangling parent is an error naming the offending feature.
#[test]
fn a_dangling_parent_is_refused_and_names_the_feature() {
    let text = r#"(version: 1, next_id: 2, features: {
        0: (kind: Poi, geometry: Point((x: 1.0, y: 1.0)), label: "orphan", parent: Some(9)),
    })"#;

    let error = World::from_ron(text, nowhere()).expect_err("a parent that is not there");

    let WorldError::BadParent { source, .. } = error else {
        panic!("expected a parent problem, got {error}");
    };
    assert_eq!(
        source,
        ParentProblem::Dangling {
            child: FeatureId(0),
            parent: FeatureId(9),
        }
    );
    assert!(source.to_string().contains("feature 0"));
}

// A feature inside itself is the shortest cycle there is, and the easiest to miss.
#[test]
fn a_feature_that_is_its_own_parent_is_refused() {
    let text = r#"(version: 1, next_id: 1, features: {
        0: (kind: Poi, geometry: Point((x: 1.0, y: 1.0)), label: "ouroboros", parent: Some(0)),
    })"#;

    let error = World::from_ron(text, nowhere()).expect_err("a self-parent");

    let WorldError::BadParent { source, .. } = error else {
        panic!("expected a parent problem, got {error}");
    };
    assert_eq!(
        source,
        ParentProblem::Cycle {
            feature: FeatureId(0)
        }
    );
}

// The case a walk that stops when it "reaches the feature again" runs forever on: the
// scan starts at 0, which is outside the 1 -> 2 -> 1 cycle it walks into.
#[test]
fn a_cycle_the_scan_starts_outside_of_is_refused_rather_than_hung() {
    let text = r#"(version: 1, next_id: 3, features: {
        0: (kind: Poi, geometry: Point((x: 0.0, y: 0.0)), label: "outside", parent: Some(1)),
        1: (kind: Poi, geometry: Point((x: 1.0, y: 1.0)), label: "in the loop", parent: Some(2)),
        2: (kind: Poi, geometry: Point((x: 2.0, y: 2.0)), label: "also in it", parent: Some(1)),
    })"#;

    let error = World::from_ron(text, nowhere()).expect_err("a cycle reachable from 0");

    assert!(matches!(
        error,
        WorldError::BadParent {
            source: ParentProblem::Cycle { .. },
            ..
        }
    ));
}

// Every feature in a long chain settles once, so the scan is proportional to the
// document rather than to chain length times feature count.
#[test]
fn a_long_parent_chain_loads() {
    let mut features = String::new();
    for id in 0..2000u64 {
        let parent = if id == 0 {
            String::new()
        } else {
            format!(", parent: Some({})", id - 1)
        };
        features.push_str(&format!(
            "{id}: (kind: Poi, geometry: Point((x: 0.0, y: 0.0)), label: \"link\"{parent}),\n"
        ));
    }
    let text = format!("(version: 1, next_id: 2000, features: {{ {features} }})");

    let world = World::from_ron(&text, nowhere()).expect("a deep chain is not a cycle");

    assert_eq!(world.len(), 2000);
}

// A coordinate that cannot be compared with itself would make the round-trip guarantee
// false while every equality test still passed, so it is refused at the door.
#[test]
fn a_non_finite_coordinate_is_refused_rather_than_round_tripped() {
    let text = r#"(version: 1, next_id: 1, features: {
        0: (kind: Poi, geometry: Point((x: NaN, y: 1.0)), label: "nowhere"),
    })"#;

    let error = World::from_ron(text, nowhere()).expect_err("NaN parses, so it must be refused");

    assert!(matches!(
        error,
        WorldError::NonFiniteCoordinate {
            feature: FeatureId(0)
        }
    ));
}

// A shape with too few vertices to be that shape is refused on load, not only when an
// edit would create one — otherwise a hand-edited file could hold one indefinitely.
#[test]
fn a_degenerate_geometry_is_refused_on_load() {
    let text = r#"(version: 1, next_id: 1, features: {
        0: (kind: Territory, geometry: Polygon([(x: 0.0, y: 0.0), (x: 1.0, y: 1.0)]), label: "flat"),
    })"#;

    let error = World::from_ron(text, nowhere()).expect_err("a two-vertex polygon");

    assert!(matches!(
        error,
        WorldError::DegenerateGeometry {
            feature: FeatureId(0),
            least: 3,
            have: 2,
            ..
        }
    ));
}

// A note path is handed to zk as an argument later, so a document may not smuggle one
// that climbs out of the notebook.
#[test]
fn a_note_path_that_escapes_the_notebook_is_refused() {
    let text = r#"(version: 1, next_id: 1, features: {
        0: (kind: Poi, geometry: Point((x: 0.0, y: 0.0)), label: "sneaky", note: Some("../../.ssh/id_rsa")),
    })"#;

    let error = World::from_ron(text, nowhere()).expect_err("a climbing note path");

    assert!(matches!(
        error,
        WorldError::BadNotePath {
            feature: FeatureId(0),
            ..
        }
    ));
}

// The counter is what makes an id permanent, so a hand-edited file that lowered it must
// not be able to hand out an id a live feature already holds.
#[test]
fn next_id_is_raised_past_the_features_a_document_holds() {
    let text = r#"(version: 1, next_id: 0, features: {
        7: (kind: Poi, geometry: Point((x: 0.0, y: 0.0)), label: "seven"),
    })"#;

    let mut world = World::from_ron(text, nowhere()).expect("load");

    assert_eq!(world.fresh_id(), FeatureId(8));
}

// A save must not be able to destroy the document that is already there, which is the
// one unrecoverable failure in this crate.
#[test]
fn a_save_leaves_no_temporary_behind() {
    let (world, _) = populated();
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("world.ron");

    world.save(&path).expect("first save");
    world.save(&path).expect("saving over an existing document");

    let leftovers: Vec<String> = std::fs::read_dir(tmp.path())
        .expect("read the directory")
        .map(|entry| entry.expect("an entry").file_name().to_string_lossy().into_owned())
        .filter(|name| name != "world.ron")
        .collect();
    assert!(leftovers.is_empty(), "left behind {leftovers:?}");
}

// Children are what a delete has to ask about and what the zoom reveal in a city is
// driven by, so the world answers it directly rather than by exposing the map.
#[test]
fn children_of_names_what_hangs_off_a_feature() {
    let (world, ids) = populated();
    let city = ids[2];

    let children: Vec<FeatureId> = world.children_of(city).collect();

    assert_eq!(children, vec![ids[3]]);
    assert_eq!(world.children_of(ids[0]).count(), 0);
}

