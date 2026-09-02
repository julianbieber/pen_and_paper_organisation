//! What a position lands on, what a box catches, and where a vertex snaps — the decisions
//! the editor delegates rather than makes, so all of them are checked without a window.
//!
//! Every hit test takes a predicate saying what the map is currently drawing. These are
//! tests of geometry rather than of what is on screen, so they admit everything; whether a
//! feature is drawn at all is decided in `campaign::lod` and checked there.

use campaign::feature::{CellPoint, Feature, FeatureId, FeatureKind, Geometry};
use campaign::pick::{self, Hit};
use campaign::world::World;
use campaign::{Edit, gesture};

fn at(x: f32, y: f32) -> CellPoint {
    CellPoint::new(x, y)
}

fn anything(_: FeatureId) -> bool {
    true
}

fn feature(kind: FeatureKind, geometry: Geometry) -> Feature {
    Feature::plain(kind, geometry)
}

fn add(world: &mut World, feature: Feature) -> FeatureId {
    let id = world.fresh_id();
    Edit::Add { id, feature }
        .apply(world)
        .expect("the fixture must be addable, or it tests nothing");
    id
}

fn square(x: f32, y: f32, side: f32) -> Geometry {
    Geometry::Polygon(vec![
        at(x, y),
        at(x + side, y),
        at(x + side, y + side),
        at(x, y + side),
    ])
}

// The rule the whole press table rests on: overlapping targets resolve to the one that
// took the most aim, or a click meaning to grab a vertex moves the whole feature instead.
#[test]
fn a_vertex_beats_an_edge_beats_a_body() {
    let mut world = World::default();
    let polygon = add(&mut world, feature(FeatureKind::Territory, square(0.0, 0.0, 10.0)));

    let on_vertex = pick::pick(&world, at(0.1, 0.1), 1.0, &anything).expect("a vertex is there");
    assert_eq!(on_vertex.hit, Hit::Vertex);
    assert_eq!(on_vertex.feature, polygon);

    let on_edge = pick::pick(&world, at(5.0, 0.2), 1.0, &anything).expect("an edge is there");
    assert_eq!(on_edge.hit, Hit::Edge);

    let inside = pick::pick(&world, at(5.0, 5.0), 1.0, &anything).expect("the body is there");
    assert_eq!(inside.hit, Hit::Body);
}

// A polyline has no body, so a press on its middle must answer Edge. Answering nothing
// would make every polyline on the map unselectable.
#[test]
fn a_press_on_a_polylines_middle_lands_on_an_edge() {
    let mut world = World::default();
    let road = add(
        &mut world,
        feature(
            FeatureKind::Road,
            Geometry::Polyline(vec![at(0.0, 0.0), at(10.0, 0.0), at(20.0, 0.0)]),
        ),
    );

    let landing = pick::pick(&world, at(5.0, 0.1), 0.5, &anything).expect("the road is under the pointer");
    assert_eq!(landing.feature, road);
    assert_eq!(landing.hit, Hit::Edge);
}

// The index an edge reports is handed straight to InsertVertex, so it must split the edge
// that was actually clicked rather than a neighbouring one.
#[test]
fn an_edges_index_inserts_into_that_edge() {
    let mut world = World::default();
    let road = add(
        &mut world,
        feature(
            FeatureKind::Road,
            Geometry::Polyline(vec![at(0.0, 0.0), at(10.0, 0.0), at(20.0, 0.0)]),
        ),
    );

    let landing = pick::pick(&world, at(15.0, 0.0), 0.5, &anything).expect("the second segment");
    assert_eq!(landing.index, 2);

    Edit::InsertVertex {
        id: road,
        index: landing.index,
        at: at(15.0, 0.0),
    }
    .apply(&mut world)
    .expect("the reported index must be insertable");

    let vertices = world.feature(road).unwrap().geometry.vertices();
    assert_eq!(vertices[2], at(15.0, 0.0));
    assert_eq!(vertices[3], at(20.0, 0.0));
}

// A tavern dropped in a city that sits in a kingdom belongs to the city. The smallest
// enclosing polygon is the whole rule, and getting it backwards parents every building to
// the realm.
#[test]
fn the_smallest_enclosing_settlement_wins() {
    let mut world = World::default();
    let _realm = add(&mut world, feature(FeatureKind::Territory, square(0.0, 0.0, 100.0)));
    let city = add(&mut world, feature(FeatureKind::Settlement, square(10.0, 10.0, 20.0)));
    let _hamlet = add(&mut world, feature(FeatureKind::Settlement, square(50.0, 50.0, 5.0)));

    assert_eq!(pick::enclosing_settlement(&world, at(15.0, 15.0)), Some(city));
    assert_eq!(pick::enclosing_settlement(&world, at(90.0, 90.0)), None);
}

// A territory is not a parent: auto-parenting a POI to the kingdom it stands in would make
// deleting that kingdom ask what to do with every POI on the map.
#[test]
fn a_territory_never_becomes_a_parent_by_enclosure() {
    let mut world = World::default();
    add(&mut world, feature(FeatureKind::Territory, square(0.0, 0.0, 100.0)));
    add(&mut world, feature(FeatureKind::Landcover, square(0.0, 0.0, 50.0)));

    assert_eq!(pick::enclosing_settlement(&world, at(10.0, 10.0)), None);
}

// Box select is "wholly inside", so a road merely crossing the box is left alone — the
// rule a GM can predict without seeing where every vertex is.
#[test]
fn a_box_catches_only_what_lies_wholly_inside_it() {
    let mut world = World::default();
    let inside = add(&mut world, feature(FeatureKind::Poi, Geometry::Point(at(5.0, 5.0))));
    let crossing = add(
        &mut world,
        feature(
            FeatureKind::Road,
            Geometry::Polyline(vec![at(-50.0, 5.0), at(50.0, 5.0)]),
        ),
    );

    let caught = pick::within(&world, at(0.0, 0.0), at(10.0, 10.0), &anything);
    assert!(caught.contains(&inside));
    assert!(!caught.contains(&crossing));
}

// The two cases the issue names: a road meeting a road, and a road meeting a city. Both
// have to be reachable or the GM aligns endpoints by eye for ever.
#[test]
fn a_placed_vertex_snaps_to_a_road_endpoint_and_to_a_settlement() {
    let mut world = World::default();
    let road = add(
        &mut world,
        feature(
            FeatureKind::Road,
            Geometry::Polyline(vec![at(0.0, 0.0), at(10.0, 0.0)]),
        ),
    );
    let town = add(
        &mut world,
        feature(FeatureKind::Settlement, Geometry::Point(at(40.0, 0.0))),
    );

    let onto_road = pick::snap(&world, &[], at(10.3, 0.2), 1.0, &anything);
    assert_eq!(onto_road.at, at(10.0, 0.0));
    assert_eq!(onto_road.onto, Some(road));

    let onto_town = pick::snap(&world, &[], at(40.2, 0.1), 1.0, &anything);
    assert_eq!(onto_town.onto, Some(town));
}

// A settlement drawn as a point is as much a target as one drawn as a polygon: what makes
// it a target is its kind. A road's own middle vertex is not a target at all.
#[test]
fn what_makes_a_snap_target_is_its_kind_not_its_shape() {
    let mut world = World::default();
    add(
        &mut world,
        feature(
            FeatureKind::Road,
            Geometry::Polyline(vec![at(0.0, 0.0), at(10.0, 0.0), at(20.0, 0.0)]),
        ),
    );

    assert!(!pick::snap(&world, &[], at(10.1, 0.0), 1.0, &anything).landed());
    assert!(pick::snap(&world, &[], at(20.1, 0.0), 1.0, &anything).landed());
}

// Closing a polygon is a snap onto the draft's own first vertex, and it is what finishes
// the shape — so it must be reported distinctly from snapping onto another feature.
#[test]
fn snapping_onto_the_drafts_own_first_vertex_closes_it() {
    let world = World::default();
    let drafted = [at(0.0, 0.0), at(10.0, 0.0), at(10.0, 10.0)];

    let closing = pick::snap(&world, &drafted, at(0.2, 0.1), 1.0, &anything);
    assert!(closing.closes);
    assert_eq!(closing.at, at(0.0, 0.0));
    assert_eq!(closing.onto, None);
}

// Snapping never moves a vertex that had no target, or every placement drifts towards
// whatever happens to be nearest on the map.
#[test]
fn a_position_with_nothing_near_it_comes_back_unchanged() {
    let mut world = World::default();
    add(
        &mut world,
        feature(FeatureKind::Settlement, Geometry::Point(at(0.0, 0.0))),
    );

    let snap = pick::snap(&world, &[], at(500.0, 500.0), 1.0, &anything);
    assert_eq!(snap.at, at(500.0, 500.0));
    assert!(!snap.landed());
}

// A selection can name a feature an undo has since removed; the drag must skip it rather
// than panic or refuse the whole gesture.
#[test]
fn a_drag_skips_a_feature_the_world_no_longer_holds() {
    let mut world = World::default();
    let kept = add(&mut world, feature(FeatureKind::Poi, Geometry::Point(at(1.0, 1.0))));
    let gone = FeatureId(999);

    let moved = gesture::dragged(&world, &[kept, gone], at(2.0, 3.0));
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].1[0], at(3.0, 4.0));
}
