//! What an edit does to a world, what it refuses, and the one property everything else
//! rests on: applying an edit and then the edit it handed back leaves the world exactly
//! as it was.

use std::collections::BTreeSet;

use campaign::brush::TileChange;
use campaign::edit::{Edit, EditError};
use campaign::feature::{CellPoint, Feature, FeatureId, FeatureKind, Geometry};
use campaign::grid::TileGrid;
use campaign::tiles::DungeonTile;
use campaign::world::{ParentProblem, World};

fn point(x: f32, y: f32) -> Geometry {
    Geometry::Point(CellPoint::new(x, y))
}

fn feature(kind: FeatureKind, geometry: Geometry, label: &str) -> Feature {
    Feature {
        label: label.to_owned(),
        ..Feature::plain(kind, geometry)
    }
}

fn add(world: &mut World, feature: Feature) -> FeatureId {
    let id = world.fresh_id();
    Edit::Add { id, feature }
        .apply(world)
        .expect("the fixture must be addable, or it tests nothing");
    id
}

struct Fixture {
    world: World,
    poi: FeatureId,
    road: FeatureId,
    city: FeatureId,
    tavern: FeatureId,
}

fn fixture() -> Fixture {
    let mut world = World::default();
    let poi = add(
        &mut world,
        feature(FeatureKind::Poi, point(3.5, -2.25), "a standing stone"),
    );
    let road = add(
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
    );
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
    let mut inn = feature(FeatureKind::Poi, point(34.0, 33.0), "the Eel");
    inn.parent = Some(city);
    inn.note = Some("places/the-eel.md".to_owned());
    let tavern = add(&mut world, inn);

    Fixture {
        world,
        poi,
        road,
        city,
        tavern,
    }
}

fn variant_name(edit: &Edit) -> &'static str {
    match edit {
        Edit::Add { .. } => "Add",
        Edit::Delete { .. } => "Delete",
        Edit::MoveVertex { .. } => "MoveVertex",
        Edit::InsertVertex { .. } => "InsertVertex",
        Edit::RemoveVertex { .. } => "RemoveVertex",
        Edit::SetKind { .. } => "SetKind",
        Edit::SetLabel { .. } => "SetLabel",
        Edit::SetNote { .. } => "SetNote",
        Edit::SetParent { .. } => "SetParent",
        Edit::SetRank { .. } => "SetRank",
        Edit::SetMaxCellsPerPixel { .. } => "SetMaxCellsPerPixel",
        Edit::SetDungeon { .. } => "SetDungeon",
        Edit::PaintTiles { .. } => "PaintTiles",
        Edit::Batch(_) => "Batch",
    }
}

const EVERY_VARIANT: [&str; 14] = [
    "Add",
    "Delete",
    "MoveVertex",
    "InsertVertex",
    "RemoveVertex",
    "SetKind",
    "SetLabel",
    "SetNote",
    "SetParent",
    "SetRank",
    "SetMaxCellsPerPixel",
    "SetDungeon",
    "PaintTiles",
    "Batch",
];

fn assert_inverts(world: &mut World, edit: Edit) {
    let before = world.clone();
    let before_bytes = before.to_ron().expect("serialize");

    let inverse = edit
        .clone()
        .apply(world)
        .unwrap_or_else(|error| panic!("{} was refused: {error}", variant_name(&edit)));
    inverse
        .apply(world)
        .unwrap_or_else(|error| panic!("the inverse of {} was refused: {error}", variant_name(&edit)));

    assert_eq!(*world, before, "{} did not invert", variant_name(&edit));
    assert_eq!(
        world.to_ron().expect("serialize"),
        before_bytes,
        "{} inverted by value but not byte for byte",
        variant_name(&edit)
    );
}

// Acceptance criterion 1: for every variant, apply-then-apply-inverse is the identity,
// asserted both by value and as the bytes the world would be saved as. The match in
// `variant_name` is exhaustive, so a further variant fails to compile rather than slipping
// through this untested.
#[test]
fn every_edit_variant_inverts_to_the_exact_prior_state() {
    let f = fixture();
    let spare = f.world.clone().features().count() as u64;

    let cases = vec![
        Edit::Add {
            id: FeatureId(spare + 100),
            feature: feature(FeatureKind::River, point(1.0, 2.0), "the Eel Water"),
        },
        Edit::Delete { id: f.poi },
        Edit::MoveVertex {
            id: f.road,
            index: 1,
            to: CellPoint::new(11.0, 1.5),
        },
        Edit::InsertVertex {
            id: f.road,
            index: 3,
            at: CellPoint::new(30.0, 6.0),
        },
        Edit::RemoveVertex {
            id: f.road,
            index: 0,
        },
        Edit::SetKind {
            id: f.poi,
            kind: FeatureKind::DungeonEntry,
        },
        Edit::SetLabel {
            id: f.city,
            label: "Riverford-on-Eel".to_owned(),
        },
        Edit::SetNote {
            id: f.poi,
            note: Some("places/standing-stone.md".to_owned()),
        },
        Edit::SetParent {
            id: f.poi,
            parent: Some(f.city),
        },
        Edit::SetRank {
            id: f.city,
            rank: Some(campaign::Rank::City),
        },
        Edit::SetMaxCellsPerPixel {
            id: f.tavern,
            scale: Some(0.25),
        },
        Edit::SetDungeon {
            id: f.poi,
            dungeon: Some("barrow.ron".to_owned()),
        },
        Edit::Batch(vec![
            Edit::SetLabel {
                id: f.poi,
                label: "the leaning stone".to_owned(),
            },
            Edit::MoveVertex {
                id: f.road,
                index: 0,
                to: CellPoint::new(-1.0, -1.0),
            },
        ]),
    ];

    let mut covered = BTreeSet::new();
    for edit in cases {
        covered.insert(variant_name(&edit));
        let mut world = f.world.clone();
        assert_inverts(&mut world, edit);
    }

    let mut dungeon = World::on_a_grid(TileGrid::new(8, 8, 1.5).expect("a legal grid"));
    let paint = Edit::PaintTiles {
        changes: vec![TileChange {
            x: 1,
            y: 2,
            tile: DungeonTile::Floor,
        }],
    };
    covered.insert(variant_name(&paint));
    assert_inverts(&mut dungeon, paint);

    assert_eq!(
        covered,
        BTreeSet::from(EVERY_VARIANT),
        "every variant must have a case"
    );
}

// Setting a value to what it already is still produces an inverse, so the symmetry holds
// for a no-op rather than being a special case the editor has to know about.
#[test]
fn setting_a_value_to_what_it_already_is_still_inverts() {
    let f = fixture();
    let mut world = f.world.clone();

    assert_inverts(
        &mut world,
        Edit::SetLabel {
            id: f.city,
            label: "Riverford".to_owned(),
        },
    );
    assert_inverts(
        &mut world,
        Edit::SetParent {
            id: f.tavern,
            parent: Some(f.city),
        },
    );
}

// Deleting a feature that is not the last one must put it back where it was, which an
// insertion-ordered map would not do — the bytes are what catches that.
#[test]
fn deleting_a_feature_in_the_middle_restores_its_position() {
    let f = fixture();
    let mut world = f.world.clone();

    assert_inverts(&mut world, Edit::Delete { id: f.road });
}

// Both notions of "exactly as it was" are asserted because they genuinely differ: -0.0
// and 0.0 are equal, and serialize to different bytes.
#[test]
fn negative_zero_is_equal_by_value_and_different_on_disk() {
    let f = fixture();
    let mut world = f.world.clone();

    assert_inverts(
        &mut world,
        Edit::MoveVertex {
            id: f.road,
            index: 0,
            to: CellPoint::new(-0.0, -0.0),
        },
    );

    assert_eq!(CellPoint::new(-0.0, 0.0), CellPoint::new(0.0, 0.0));
    let mut negative = f.world.clone();
    Edit::MoveVertex {
        id: f.road,
        index: 0,
        to: CellPoint::new(-0.0, 0.0),
    }
    .apply(&mut negative)
    .expect("move to negative zero");
    assert_ne!(
        negative.to_ron().expect("serialize"),
        f.world.to_ron().expect("serialize"),
        "-0.0 is equal by value but is not the same document on disk"
    );
}

// Appending is the ordinary way to extend a polyline, so the vertex count itself must be
// a legal insert index even though it is not a legal move or remove index.
#[test]
fn a_vertex_may_be_inserted_at_the_end_but_not_moved_there() {
    let f = fixture();
    let mut world = f.world.clone();
    let len = world.feature(f.road).expect("the road").geometry.len();

    Edit::InsertVertex {
        id: f.road,
        index: len,
        at: CellPoint::new(99.0, 99.0),
    }
    .apply(&mut world)
    .expect("appending at the vertex count");
    assert_eq!(world.feature(f.road).expect("the road").geometry.len(), len + 1);

    let error = Edit::MoveVertex {
        id: f.road,
        index: len + 1,
        to: CellPoint::new(0.0, 0.0),
    }
    .apply(&mut world)
    .expect_err("moving past the end");
    assert!(matches!(error, EditError::VertexOutOfBounds { .. }));
}

// A point has exactly one vertex: it may be moved, and the shape may not gain or lose one.
#[test]
fn a_point_may_be_moved_but_not_grown_or_shrunk() {
    let f = fixture();
    let mut world = f.world.clone();

    assert_inverts(
        &mut world,
        Edit::MoveVertex {
            id: f.poi,
            index: 0,
            to: CellPoint::new(9.0, 9.0),
        },
    );

    for edit in [
        Edit::InsertVertex {
            id: f.poi,
            index: 0,
            at: CellPoint::new(1.0, 1.0),
        },
        Edit::RemoveVertex {
            id: f.poi,
            index: 0,
        },
    ] {
        let error = edit.apply(&mut world).expect_err("a point is not a vertex list");
        assert!(matches!(error, EditError::NotAVertexList { .. }));
    }
}

// The shape's minimum is a floor an edit cannot go under, so a saved document can never
// hold a polygon that is not one.
#[test]
fn a_geometry_cannot_be_shrunk_below_its_shape() {
    let f = fixture();
    let mut world = f.world.clone();

    let error = Edit::RemoveVertex {
        id: f.city,
        index: 0,
    }
    .apply(&mut world)
    .expect_err("a triangle is already the smallest polygon");

    assert!(matches!(
        error,
        EditError::TooFewVertices {
            least: 3,
            have: 2,
            ..
        }
    ));
}

// A delete never orphans and never cascades: it is refused, naming the children, so the
// decision about them is the GM's rather than the model's.
#[test]
fn deleting_a_feature_with_children_is_refused_and_names_them() {
    let f = fixture();
    let mut world = f.world.clone();

    let error = Edit::Delete { id: f.city }
        .apply(&mut world)
        .expect_err("the city still holds the tavern");

    let EditError::HasChildren { feature, children } = error else {
        panic!("expected HasChildren");
    };
    assert_eq!(feature, f.city);
    assert_eq!(children, vec![f.tavern]);
}

// Every rule load enforces is enforced here too, or an add could build a document that
// saves cleanly and then refuses to open.
#[test]
fn an_add_cannot_build_a_world_that_would_not_load() {
    let f = fixture();
    let spare = FeatureId(500);

    let mut dangling = feature(FeatureKind::Poi, point(0.0, 0.0), "orphan");
    dangling.parent = Some(FeatureId(999));
    let mut world = f.world.clone();
    assert!(matches!(
        Edit::Add {
            id: spare,
            feature: dangling
        }
        .apply(&mut world),
        Err(EditError::BadParent(ParentProblem::Dangling { .. }))
    ));

    let mut degenerate = feature(
        FeatureKind::Territory,
        Geometry::Polygon(vec![CellPoint::new(0.0, 0.0), CellPoint::new(1.0, 1.0)]),
        "flat",
    );
    degenerate.label = "flat".to_owned();
    let mut world = f.world.clone();
    assert!(matches!(
        Edit::Add {
            id: spare,
            feature: degenerate
        }
        .apply(&mut world),
        Err(EditError::TooFewVertices { .. })
    ));

    let mut infinite = feature(FeatureKind::Poi, point(f32::INFINITY, 0.0), "nowhere");
    infinite.note = None;
    let mut world = f.world.clone();
    assert!(matches!(
        Edit::Add {
            id: spare,
            feature: infinite
        }
        .apply(&mut world),
        Err(EditError::NonFiniteCoordinate { .. })
    ));

    let mut escaping = feature(FeatureKind::Poi, point(0.0, 0.0), "sneaky");
    escaping.note = Some("../../.ssh/id_rsa".to_owned());
    let mut world = f.world.clone();
    assert!(matches!(
        Edit::Add {
            id: spare,
            feature: escaping
        }
        .apply(&mut world),
        Err(EditError::BadNotePath { .. })
    ));
}

// A note path reaches zk as an argument, so the check lives here rather than in the UI —
// which is what makes a control socket inherit it.
#[test]
fn a_note_path_is_checked_where_it_is_set() {
    let f = fixture();
    let mut world = f.world.clone();

    for bad in ["", "/etc/passwd", "../outside.md", "-oProxyCommand=x"] {
        let error = Edit::SetNote {
            id: f.poi,
            note: Some(bad.to_owned()),
        }
        .apply(&mut world)
        .expect_err("a note path zk must not be handed");
        assert!(
            matches!(error, EditError::BadNotePath { .. }),
            "{bad:?} was refused as {error}"
        );
    }

    Edit::SetNote {
        id: f.poi,
        note: Some("places/nested/stone.md".to_owned()),
    }
    .apply(&mut world)
    .expect("an ordinary relative path inside the notebook");
}

// A feature cannot be put inside itself, directly or round a chain.
#[test]
fn reparenting_into_a_cycle_is_refused() {
    let f = fixture();
    let mut world = f.world.clone();

    let error = Edit::SetParent {
        id: f.city,
        parent: Some(f.tavern),
    }
    .apply(&mut world)
    .expect_err("the tavern is already inside the city");
    assert!(matches!(
        error,
        EditError::BadParent(ParentProblem::Cycle { .. })
    ));

    let error = Edit::SetParent {
        id: f.poi,
        parent: Some(f.poi),
    }
    .apply(&mut world)
    .expect_err("a feature inside itself");
    assert!(matches!(
        error,
        EditError::BadParent(ParentProblem::Cycle { .. })
    ));
}

// The subject and the parent are told apart by which one is missing, so a caller can
// assert on the variant rather than on "one or the other".
#[test]
fn a_missing_subject_and_a_missing_parent_are_different_refusals() {
    let f = fixture();
    let mut world = f.world.clone();

    assert!(matches!(
        Edit::SetParent {
            id: FeatureId(999),
            parent: None
        }
        .apply(&mut world),
        Err(EditError::NoSuchFeature(_))
    ));
    assert!(matches!(
        Edit::SetParent {
            id: f.poi,
            parent: Some(FeatureId(999))
        }
        .apply(&mut world),
        Err(EditError::BadParent(ParentProblem::Dangling { .. }))
    ));
}

// The rule every refusal rests on, checked as bytes so nothing partial can hide in it.
#[test]
fn a_refused_edit_changes_nothing() {
    let f = fixture();
    let mut world = f.world.clone();
    let before = world.to_ron().expect("serialize");

    for edit in [
        Edit::Delete { id: f.city },
        Edit::Delete {
            id: FeatureId(999),
        },
        Edit::Add {
            id: f.poi,
            feature: feature(FeatureKind::Poi, point(0.0, 0.0), "clash"),
        },
        Edit::RemoveVertex {
            id: f.city,
            index: 0,
        },
        Edit::MoveVertex {
            id: f.road,
            index: 99,
            to: CellPoint::new(0.0, 0.0),
        },
        Edit::SetParent {
            id: f.city,
            parent: Some(f.tavern),
        },
    ] {
        let refusal = edit.clone().apply(&mut world);
        assert!(refusal.is_err(), "{edit:?} should have been refused");
        assert_eq!(world.to_ron().expect("serialize"), before);
    }
}

// A batch is atomic: one refused part-way must leave nothing of the prefix behind, or
// the world ends up in a state no single edit could have produced.
#[test]
fn a_batch_refused_part_way_rolls_its_prefix_back() {
    let f = fixture();
    let mut world = f.world.clone();
    let before = world.to_ron().expect("serialize");

    let error = Edit::Batch(vec![
        Edit::SetLabel {
            id: f.poi,
            label: "renamed".to_owned(),
        },
        Edit::MoveVertex {
            id: f.road,
            index: 0,
            to: CellPoint::new(5.0, 5.0),
        },
        Edit::Delete { id: f.city },
    ])
    .apply(&mut world)
    .expect_err("the city still holds the tavern");

    assert!(matches!(error, EditError::HasChildren { .. }));
    assert_eq!(
        world.to_ron().expect("serialize"),
        before,
        "the two edits before the refusal must have been undone"
    );
}

// Deleting a settlement together with what it holds is the case a batch exists for: the
// children go first, so the parent is deletable by the time its turn comes.
#[test]
fn a_batch_deletes_a_settlement_and_its_children_together() {
    let f = fixture();
    let mut world = f.world.clone();

    assert_inverts(
        &mut world,
        Edit::Batch(vec![
            Edit::Delete { id: f.tavern },
            Edit::Delete { id: f.city },
        ]),
    );

    Edit::Batch(vec![
        Edit::Delete { id: f.tavern },
        Edit::Delete { id: f.city },
    ])
    .apply(&mut world)
    .expect("children first, then the parent");
    assert!(world.feature(f.city).is_none());
    assert!(world.feature(f.tavern).is_none());
}

// A batch nests and an empty one is legal, so a caller building one from a selection
// never has to special-case the count.
#[test]
fn a_batch_nests_and_may_be_empty() {
    let f = fixture();
    let mut world = f.world.clone();

    assert_inverts(&mut world, Edit::Batch(vec![]));
    assert_inverts(
        &mut world,
        Edit::Batch(vec![
            Edit::Batch(vec![Edit::SetLabel {
                id: f.poi,
                label: "nested".to_owned(),
            }]),
            Edit::Batch(vec![]),
        ]),
    );
}

// An edit is a value a socket will parse, so it has to survive being written and read.
#[test]
fn an_edit_round_trips_through_ron() {
    let edit = Edit::Batch(vec![
        Edit::Add {
            id: FeatureId(4),
            feature: feature(FeatureKind::Landcover, point(1.0, 2.0), "Mirkwood"),
        },
        Edit::SetParent {
            id: FeatureId(4),
            parent: None,
        },
    ]);

    let text = ron::ser::to_string(&edit).expect("serialize");
    let back: Edit = ron::from_str(&text).expect("parse");

    assert_eq!(back, edit);
}

// The edit gate and the load gate must refuse the same things, or an edit could build a
// world that would be refused if it were saved and opened again.
#[test]
fn an_unusable_reveal_scale_is_refused_where_it_is_set() {
    let f = fixture();
    let mut world = f.world.clone();

    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let refusal = Edit::SetMaxCellsPerPixel {
            id: f.poi,
            scale: Some(scale),
        }
        .apply(&mut world)
        .expect_err(&format!("a reveal scale of {scale} must be refused"));
        assert!(matches!(refusal, EditError::BadRevealScale { .. }));
    }

    assert_eq!(world, f.world, "a refused edit changed the world");
}

// An Add carries the field too, so the gate has to be on that path as well — otherwise a
// scripted run could add a feature carrying a scale no SetMaxCellsPerPixel would accept.
#[test]
fn an_add_carrying_an_unusable_reveal_scale_is_refused() {
    let f = fixture();
    let mut world = f.world.clone();
    let id = world.fresh_id();

    let refusal = Edit::Add {
        id,
        feature: Feature {
            max_cells_per_pixel: Some(f32::NAN),
            ..feature(FeatureKind::Poi, point(1.0, 1.0), "a cairn")
        },
    }
    .apply(&mut world)
    .expect_err("an add carrying a NaN reveal scale must be refused");
    assert!(matches!(refusal, EditError::BadRevealScale { .. }));
    assert!(world.feature(id).is_none());
}

// The property every tile edit rests on, stated the way the geometry edits state theirs:
// applying a paint and then the paint it handed back leaves the grid exactly as it was.
#[test]
fn painting_and_then_undoing_it_leaves_the_grid_as_it_was() {
    let mut world = World::on_a_grid(TileGrid::new(8, 8, 1.5).expect("a legal grid"));
    let before = world.to_ron().expect("a world must serialize");

    let changes = campaign::brush::cells(
        world.grid().expect("a dungeon has a grid"),
        campaign::brush::Brush::Room,
        &[(1, 1), (5, 5)],
        DungeonTile::Floor,
    );
    let inverse = Edit::PaintTiles { changes }
        .apply(&mut world)
        .expect("a room on an empty grid must land");
    assert_ne!(world.to_ron().expect("serializes"), before);

    inverse.apply(&mut world).expect("the inverse must apply");
    assert_eq!(world.to_ron().expect("serializes"), before);
}

// One stroke is one entry on the undo stack however many cells it covers — which is the
// whole reason a paint is its own variant rather than a Batch holding a change per cell.
#[test]
fn a_stroke_of_any_size_is_one_edit() {
    let world = World::on_a_grid(TileGrid::new(32, 32, 1.5).expect("a legal grid"));
    let changes = campaign::brush::cells(
        world.grid().expect("a dungeon has a grid"),
        campaign::brush::Brush::Flood,
        &[(0, 0)],
        DungeonTile::Floor,
    );
    assert_eq!(changes.len(), 1024, "the fill covers the whole grid");

    let mut document = campaign::Document::new(world.clone());
    document
        .apply(Edit::PaintTiles { changes })
        .expect("the fill must land");
    assert_eq!(document.undo_depth(), 1);

    document.undo().expect("undoing must work");
    assert_eq!(
        document.world().to_ron().expect("serializes"),
        world.to_ron().expect("serializes"),
        "one press must put the whole stroke back"
    );
}

// A paint against the world map is refused rather than silently ignored: the world map is
// drawn on the terrain and has no grid, and a socket or a redo can reach this.
#[test]
fn painting_a_document_with_no_grid_is_refused() {
    let f = fixture();
    let mut world = f.world.clone();
    let refusal = Edit::PaintTiles {
        changes: vec![TileChange {
            x: 0,
            y: 0,
            tile: DungeonTile::Floor,
        }],
    }
    .apply(&mut world)
    .expect_err("the world map has no grid to paint");
    assert!(matches!(refusal, EditError::NoGrid));
    assert_eq!(world, f.world, "a refused edit changed the world");
}

// Every cell is bounded before any is written, so a paint whose last cell is off the grid
// leaves the grid untouched rather than half-painted.
#[test]
fn a_paint_naming_a_cell_off_the_grid_writes_nothing() {
    let mut world = World::on_a_grid(TileGrid::new(4, 4, 1.5).expect("a legal grid"));
    let before = world.clone();

    let refusal = Edit::PaintTiles {
        changes: vec![
            TileChange {
                x: 0,
                y: 0,
                tile: DungeonTile::Floor,
            },
            TileChange {
                x: 9,
                y: 0,
                tile: DungeonTile::Floor,
            },
        ],
    }
    .apply(&mut world)
    .expect_err("a cell off the grid must be refused");
    assert!(matches!(refusal, EditError::CellOutOfGrid { x: 9, y: 0, .. }));
    assert_eq!(world, before, "a refused paint painted the first cell");
}

// An entry on the undo stack must always undo something, so a stroke that lands entirely
// on cells already carrying the tile is refused rather than pushed.
#[test]
fn a_paint_that_changes_nothing_is_refused() {
    let mut world = World::on_a_grid(TileGrid::new(4, 4, 1.5).expect("a legal grid"));
    let refusal = Edit::PaintTiles {
        changes: vec![TileChange {
            x: 1,
            y: 1,
            tile: DungeonTile::Empty,
        }],
    }
    .apply(&mut world)
    .expect_err("painting the tile already there must be refused");
    assert!(matches!(refusal, EditError::EmptyPaint));
}

// A caller that names one cell twice must not produce an inverse that depends on the order
// it is applied in.
#[test]
fn a_paint_naming_one_cell_twice_keeps_the_last_word() {
    let mut world = World::on_a_grid(TileGrid::new(4, 4, 1.5).expect("a legal grid"));
    let inverse = Edit::PaintTiles {
        changes: vec![
            TileChange {
                x: 1,
                y: 1,
                tile: DungeonTile::Floor,
            },
            TileChange {
                x: 1,
                y: 1,
                tile: DungeonTile::Wall,
            },
        ],
    }
    .apply(&mut world)
    .expect("the paint must land");

    assert_eq!(world.grid().unwrap().get(1, 1), Some(DungeonTile::Wall));
    let Edit::PaintTiles { changes } = &inverse else {
        panic!("the inverse of a paint is a paint");
    };
    assert_eq!(changes.len(), 1, "one cell, one entry in the inverse");
    inverse.apply(&mut world).expect("the inverse must apply");
    assert_eq!(world.grid().unwrap().get(1, 1), Some(DungeonTile::Empty));
}

// The link from an entry to its dungeon is an Edit like everything else, so it is undoable
// and it is saved with the world map.
#[test]
fn a_dungeon_link_is_set_and_undone_like_any_other_field() {
    let f = fixture();
    let mut world = f.world.clone();

    let inverse = Edit::SetDungeon {
        id: f.poi,
        dungeon: Some("crypt.ron".to_owned()),
    }
    .apply(&mut world)
    .expect("naming a dungeon must land");
    assert_eq!(
        world.feature(f.poi).unwrap().dungeon.as_deref(),
        Some("crypt.ron")
    );

    inverse.apply(&mut world).expect("the inverse must apply");
    assert_eq!(world, f.world);
}

// The name is joined to a directory and then opened for writing, so every rule World::load
// enforces about it has to be enforced here too — including on the Add path, which carries
// a whole feature and is the arm easiest to miss.
#[test]
fn a_dungeon_name_that_is_not_one_file_in_one_directory_is_refused() {
    let f = fixture();
    for name in [
        "",
        "-crypt.ron",
        "../world.ron",
        "sub/crypt.ron",
        "/etc/passwd",
        ".",
        "..",
        "crypt",
        "crypt.ron\u{0}",
    ] {
        let mut world = f.world.clone();
        let refusal = Edit::SetDungeon {
            id: f.poi,
            dungeon: Some(name.to_owned()),
        }
        .apply(&mut world)
        .expect_err(&format!("`{name}` must be refused as a dungeon name"));
        assert!(matches!(refusal, EditError::BadDungeonName { .. }));

        let mut world = f.world.clone();
        let id = world.fresh_id();
        let refusal = Edit::Add {
            id,
            feature: Feature {
                dungeon: Some(name.to_owned()),
                ..feature(FeatureKind::DungeonEntry, point(1.0, 1.0), "a stair")
            },
        }
        .apply(&mut world)
        .expect_err(&format!("an add carrying `{name}` must be refused"));
        assert!(matches!(refusal, EditError::BadDungeonName { .. }));
    }
}

// Two entries naming one file would both open and save it, so the second is refused where
// it is stored rather than discovered when work is lost.
#[test]
fn two_features_may_not_name_the_same_dungeon() {
    let f = fixture();
    let mut world = f.world.clone();
    Edit::SetDungeon {
        id: f.poi,
        dungeon: Some("crypt.ron".to_owned()),
    }
    .apply(&mut world)
    .expect("the first must land");

    let refusal = Edit::SetDungeon {
        id: f.city,
        dungeon: Some("crypt.ron".to_owned()),
    }
    .apply(&mut world)
    .expect_err("the second must be refused");
    assert!(matches!(refusal, EditError::DungeonNameInUse { .. }));
}

// A dungeon is a child of the world map and not of another dungeon: there is one place to
// come back to, so nesting is refused where it is stored.
#[test]
fn a_feature_inside_a_dungeon_may_not_name_a_dungeon() {
    let mut world = World::on_a_grid(TileGrid::new(8, 8, 1.5).expect("a legal grid"));
    let id = world.fresh_id();
    Edit::Add {
        id,
        feature: feature(FeatureKind::DungeonEntry, point(1.0, 1.0), "a stair down"),
    }
    .apply(&mut world)
    .expect("adding a feature inside a dungeon must work");

    let refusal = Edit::SetDungeon {
        id,
        dungeon: Some("deeper.ron".to_owned()),
    }
    .apply(&mut world)
    .expect_err("a dungeon inside a dungeon must be refused");
    assert!(matches!(refusal, EditError::NestedDungeon { .. }));
}
