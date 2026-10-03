//! Which terrain cells a settlement or a road draws as its own tile, checked without a
//! window.

use campaign::claims::Claims;
use campaign::edit::Edit;
use campaign::feature::{CellPoint, Feature, FeatureKind, Geometry};
use campaign::grid::TileGrid;
use campaign::tiles::{CHUNK_CELLS, TileKind};
use campaign::world::World;

fn at(x: f32, y: f32) -> CellPoint {
    CellPoint::new(x, y)
}

fn add(world: &mut World, kind: FeatureKind, geometry: Geometry) {
    let id = world.fresh_id();
    Edit::Add {
        id,
        feature: Feature::plain(kind, geometry),
    }
    .apply(world)
    .expect("the fixture must be addable, or it tests nothing");
}

fn square(low: f32, high: f32) -> Geometry {
    Geometry::Polygon(vec![
        at(low, low),
        at(high, low),
        at(high, high),
        at(low, high),
    ])
}

fn kind_at(claims: &Claims, x: u32, y: u32) -> Option<TileKind> {
    let side = CHUNK_CELLS;
    let slot = (side - 1 - y % side) * side + x % side;
    claims
        .in_chunk((x / side) as i32, (y / side) as i32)
        .iter()
        .find(|cell| cell.slot == slot)
        .map(|cell| cell.kind)
}

// A settlement claims the cells whose centres it covers and nothing outside them.
#[test]
fn a_settlement_claims_the_cells_inside_it() {
    let mut world = World::default();
    add(&mut world, FeatureKind::Settlement, square(9.5, 12.5));
    let claims = Claims::of(&world, 100, 100);

    for (x, y) in [(10, 10), (12, 12), (11, 10)] {
        assert_eq!(kind_at(&claims, x, y), Some(TileKind::City), "({x}, {y})");
    }
    assert_eq!(kind_at(&claims, 9, 10), None);
    assert_eq!(kind_at(&claims, 13, 13), None);
}

// A road claims every cell along it without gaps, a city wins where the two meet, and
// a feature of any other kind claims nothing.
#[test]
fn a_road_claims_a_connected_line_and_a_city_wins_over_it() {
    let mut world = World::default();
    add(
        &mut world,
        FeatureKind::Road,
        Geometry::Polyline(vec![at(0.0, 0.0), at(20.0, 0.0)]),
    );
    add(
        &mut world,
        FeatureKind::Settlement,
        Geometry::Polygon(vec![
            at(4.5, -0.5),
            at(6.5, -0.5),
            at(6.5, 1.5),
            at(4.5, 1.5),
        ]),
    );
    add(&mut world, FeatureKind::Landcover, square(30.0, 40.0));
    let claims = Claims::of(&world, 100, 100);

    for x in 0..=20 {
        let want = if (5..=6).contains(&x) {
            TileKind::City
        } else {
            TileKind::Road
        };
        assert_eq!(kind_at(&claims, x, 0), Some(want), "x = {x}");
    }
    assert_eq!(kind_at(&claims, 5, 1), Some(TileKind::City));
    assert_eq!(
        kind_at(&claims, 35, 35),
        None,
        "landcover is not a settlement"
    );
}

// Cells off the terrain are dropped rather than wrapped into a neighbouring chunk.
#[test]
fn claims_stop_at_the_terrains_edge() {
    let mut world = World::default();
    add(&mut world, FeatureKind::Settlement, square(-5.0, 5.0));
    let claims = Claims::of(&world, 4, 4);

    assert_eq!(kind_at(&claims, 0, 0), Some(TileKind::City));
    assert_eq!(kind_at(&claims, 3, 3), Some(TileKind::City));
    assert!(claims.in_chunk(-1, -1).is_empty());
    assert_eq!(claims.in_chunk(0, 0).len(), 16, "exactly the 4x4 terrain");
}

// A dungeon is drawn on its own grid, so nothing in it claims a terrain cell.
#[test]
fn a_dungeon_claims_nothing() {
    let mut world = World::on_a_grid(TileGrid::new(16, 16, 1.524).expect("a valid grid"));
    add(&mut world, FeatureKind::Settlement, square(1.0, 5.0));
    assert_eq!(Claims::of(&world, 100, 100), Claims::default());
}

// Moving a settlement redraws only the chunks it left and the chunks it reached.
#[test]
fn only_chunks_whose_claims_changed_differ() {
    let mut before = World::default();
    add(&mut before, FeatureKind::Settlement, square(1.0, 3.0));
    add(&mut before, FeatureKind::Settlement, square(200.0, 202.0));
    let mut after = World::default();
    add(&mut after, FeatureKind::Settlement, square(70.0, 72.0));
    add(&mut after, FeatureKind::Settlement, square(200.0, 202.0));

    let before = Claims::of(&before, 300, 300);
    let after = Claims::of(&after, 300, 300);
    assert_eq!(after.chunks_differing_from(&before), vec![(0, 0), (1, 1)]);
    assert!(after.chunks_differing_from(&after).is_empty());
}
