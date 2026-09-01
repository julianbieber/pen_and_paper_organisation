//! The single Edit an authoring gesture becomes, and the one thing that makes a removal
//! work at all: the order inside its batch.

use campaign::edit::Edit;
use campaign::feature::{CellPoint, Feature, FeatureId, FeatureKind, Geometry};
use campaign::gesture::{self, Orphans};
use campaign::world::World;

fn at(x: f32, y: f32) -> CellPoint {
    CellPoint::new(x, y)
}

fn point(world: &mut World, parent: Option<FeatureId>) -> FeatureId {
    let id = world.fresh_id();
    Edit::Add {
        id,
        feature: Feature {
            parent,
            ..Feature::plain(FeatureKind::Poi, Geometry::Point(at(0.0, 0.0)))
        },
    }
    .apply(world)
    .expect("the fixture must be addable, or it tests nothing");
    id
}

fn three_deep() -> (World, FeatureId, FeatureId, FeatureId) {
    let mut world = World::default();
    let city = point(&mut world, None);
    let tavern = point(&mut world, Some(city));
    let cellar = point(&mut world, Some(tavern));
    (world, city, tavern, cellar)
}

// Delete is refused while anything names its target as parent, and a batch refused
// part-way rolls itself back — so a cascade emitted parent-first does nothing at all
// rather than half the job. This pins the order that makes it apply.
#[test]
fn a_cascade_removes_the_deepest_first_and_applies_cleanly() {
    let (mut world, city, tavern, cellar) = three_deep();

    let edit = gesture::remove(&world, city, Orphans::Cascade);
    edit.apply(&mut world).expect("a cascade must apply in one go");

    assert!(world.feature(city).is_none());
    assert!(world.feature(tavern).is_none());
    assert!(world.feature(cellar).is_none());
    assert!(world.is_empty());
}

// Promote must re-parent every child before the delete, or the delete is refused and the
// batch unwinds. The grandparent is what the children land on.
#[test]
fn a_promote_hands_the_children_up_before_it_deletes() {
    let (mut world, city, tavern, cellar) = three_deep();

    gesture::remove(&world, tavern, Orphans::Promote)
        .apply(&mut world)
        .expect("a promote must apply in one go");

    assert!(world.feature(tavern).is_none());
    assert_eq!(world.feature(cellar).unwrap().parent, Some(city));
}

// A feature with no parent promotes its children to no parent, which must still be a
// coherent document rather than a dangling link.
#[test]
fn promoting_out_of_a_root_leaves_the_children_parentless() {
    let (mut world, city, tavern, _cellar) = three_deep();

    gesture::remove(&world, city, Orphans::Promote)
        .apply(&mut world)
        .expect("a promote at the root must apply");

    assert_eq!(world.feature(tavern).unwrap().parent, None);
}

// Selecting a parent and its own child and pressing Delete is the ordinary case, and
// emitted in id order it refuses itself.
#[test]
fn deleting_a_parent_and_its_child_together_does_not_refuse_itself() {
    let (mut world, city, tavern, cellar) = three_deep();

    gesture::remove_all(&world, &[city, tavern, cellar])
        .apply(&mut world)
        .expect("a multi-delete must apply in one go");

    assert!(world.is_empty());
}

// Only children outside the selection are a question. One whose parent and self are both
// being deleted needs no answer, and asking anyway is a prompt for nothing.
#[test]
fn only_children_outside_the_selection_raise_a_question() {
    let (world, city, tavern, cellar) = three_deep();

    assert!(gesture::outside_children(&world, &[city, tavern, cellar]).is_empty());

    let asked = gesture::outside_children(&world, &[city, tavern]);
    assert_eq!(asked, vec![(tavern, vec![cellar])]);
}

// A removal is one batch so it undoes in one press, and the inverse has to put the whole
// subtree back exactly as it was.
#[test]
fn undoing_a_cascade_restores_the_whole_subtree() {
    let (mut world, city, _tavern, _cellar) = three_deep();
    let before = world.clone();

    let inverse = gesture::remove(&world, city, Orphans::Cascade)
        .apply(&mut world)
        .expect("a cascade must apply");
    inverse.apply(&mut world).expect("its inverse must apply");

    assert_eq!(world, before);
}

// A drag is one entry however many vertices it moves, and the overlay must draw exactly
// what that entry will commit.
#[test]
fn a_translate_moves_every_vertex_by_the_same_offset_in_one_batch() {
    let mut world = World::default();
    let id = world.fresh_id();
    let vertices: Vec<CellPoint> = (0..200).map(|n| at(n as f32, 0.0)).collect();
    Edit::Add {
        id,
        feature: Feature {
            label: "Riverford".to_owned(),
            ..Feature::plain(FeatureKind::Road, Geometry::Polyline(vertices))
        },
    }
    .apply(&mut world)
    .expect("the fixture must be addable");

    let offset = at(3.0, -4.0);
    let shown = gesture::dragged(&world, &[id], offset);
    let edit = gesture::translate(&world, &[id], offset);

    let Edit::Batch(ref edits) = edit else {
        panic!("a translate is one batch");
    };
    assert_eq!(edits.len(), 200);

    edit.apply(&mut world).expect("a translate must apply");
    assert_eq!(world.feature(id).unwrap().geometry.vertices(), shown[0].1);
    assert_eq!(world.feature(id).unwrap().geometry.vertices()[0], at(3.0, -4.0));
}

// Placing a point inside a city parents it there in the same edit, so the GM never states
// a relation they already expressed by where they clicked.
#[test]
fn placing_a_point_inside_a_settlement_parents_it_in_one_edit() {
    let mut world = World::default();
    let city = world.fresh_id();
    Edit::Add {
        id: city,
        feature: Feature::plain(
            FeatureKind::Settlement,
            Geometry::Polygon(vec![at(0.0, 0.0), at(10.0, 0.0), at(10.0, 10.0)]),
        ),
    }
    .apply(&mut world)
    .expect("the city must be addable");

    let tavern = world.fresh_id();
    let edit = gesture::place(
        &world,
        tavern,
        Feature {
            label: "the Eel".to_owned(),
            parent: None,
            ..Feature::plain(FeatureKind::Poi, Geometry::Point(at(5.0, 2.0)))
        },
    );
    assert!(matches!(edit, Edit::Add { .. }), "a placement is one add");
    edit.apply(&mut world).expect("the placement must apply");

    assert_eq!(world.feature(tavern).unwrap().parent, Some(city));
}
