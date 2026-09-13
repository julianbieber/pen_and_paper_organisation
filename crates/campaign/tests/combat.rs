//! The combat map as a document: the strip order its vocabulary fixes, what a new map is
//! filled with, what it refuses, and that a stroke is exactly one press of undo.

use campaign::brush::{self, Brush, TileChange};
use campaign::combat::{CombatEdit, CombatMap, CombatProblem, DEFAULT_COMBAT_CELLS, unused_name};
use campaign::document::Document;
use campaign::edit::EditError;
use campaign::grid::{DEFAULT_METRES_PER_CELL, GridProblem, TileVocabulary};
use campaign::measure::{DistanceUnit, worth_of_grid};
use campaign::tiles::{COMBAT_TILE_COUNT, CombatTile};

fn map() -> CombatMap {
    CombatMap::new("Bridge", DEFAULT_COMBAT_CELLS, DEFAULT_COMBAT_CELLS).expect("a 30x30 map is valid")
}

fn stroke(map: &CombatMap, brush: Brush, from: (i64, i64), to: (i64, i64), tile: CombatTile) -> CombatEdit {
    CombatEdit::PaintTiles {
        changes: brush::cells(map.grid(), brush, &[from, to], tile),
    }
}

// The strip is drawn by hand and read by column, so the order the issue fixed — index 0
// Grass through 11 Floor — is the contract a redraw must keep.
#[test]
fn the_vocabulary_numbers_the_strip_in_the_issues_order() {
    let all = CombatTile::all();
    assert_eq!(all.len(), usize::from(COMBAT_TILE_COUNT));
    for (column, tile) in all.iter().enumerate() {
        assert_eq!(usize::from(tile.index()), column, "{tile:?}");
        assert_eq!(usize::from(TileVocabulary::index(*tile)), column, "{tile:?}");
    }
    assert_eq!(
        all,
        [
            CombatTile::Grass,
            CombatTile::Dirt,
            CombatTile::Road,
            CombatTile::Sand,
            CombatTile::Mud,
            CombatTile::ShallowWater,
            CombatTile::DeepWater,
            CombatTile::Tree,
            CombatTile::Bush,
            CombatTile::Boulder,
            CombatTile::Wall,
            CombatTile::Floor,
        ]
    );
    assert_eq!(CombatTile::default(), CombatTile::Grass);
    assert_eq!(<CombatTile as TileVocabulary>::WALL, CombatTile::Wall);
    assert_eq!(<CombatTile as TileVocabulary>::FLOOR, CombatTile::Floor);

    let mut labels: Vec<&str> = all.iter().map(|tile| tile.label()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), all.len(), "two tiles share a label");
}

// A new map is what one press opens, so every cell must already be the default tile and
// the grid five feet to the cell.
#[test]
fn a_new_map_is_all_grass_at_five_feet() {
    let map = map();
    assert_eq!(map.name(), "Bridge");
    assert_eq!(map.grid().cells(), 900);
    assert!(map.grid().tiles().iter().all(|tile| *tile == CombatTile::Grass));
    assert_eq!(map.grid().metres_per_cell(), DEFAULT_METRES_PER_CELL);
}

// A name is where the next issue's file name comes from, so one that yields no slug is
// refused where it is typed rather than when the map is saved.
#[test]
fn a_name_with_no_letter_or_digit_is_refused() {
    for name in ["", "   ", "!!!"] {
        assert_eq!(CombatMap::new(name, 30, 30), Err(CombatProblem::Unnamed), "{name:?}");
    }
    assert_eq!(CombatMap::new("  Ford  ", 4, 4).unwrap().name(), "Ford");
}

// The grid's own limits reach the GM through the combat map unchanged, and the name is
// checked first so the answer to a doubly wrong form does not depend on the size.
#[test]
fn a_size_a_grid_cannot_have_is_refused() {
    assert!(matches!(
        CombatMap::new("Bridge", 0, 30),
        Err(CombatProblem::Grid(GridProblem::NoExtent { .. }))
    ));
    assert!(matches!(
        CombatMap::new("Bridge", 30, 0),
        Err(CombatProblem::Grid(GridProblem::NoExtent { .. }))
    ));
    assert!(matches!(
        CombatMap::new("Bridge", 2048, 1024),
        Err(CombatProblem::Grid(GridProblem::TooManyCells { .. }))
    ));
    assert_eq!(CombatMap::new("", 0, 0), Err(CombatProblem::Unnamed));
}

// The first acceptance criterion: undoing one stroke removes that stroke and only that
// stroke, and redo puts it back.
#[test]
fn undo_takes_back_exactly_one_stroke() {
    let mut document = Document::new(map());
    let trees = stroke(document.content(), Brush::Rectangle, (2, 2), (4, 4), CombatTile::Tree);
    document.apply(trees).unwrap();
    let after_trees = document.content().clone();
    let water = stroke(document.content(), Brush::Rectangle, (3, 3), (6, 6), CombatTile::ShallowWater);
    document.apply(water).unwrap();
    assert_eq!(document.undo_depth(), 2);
    assert_eq!(document.content().grid().get(3, 3), Some(CombatTile::ShallowWater));

    assert_eq!(document.undo(), Ok(true));
    assert_eq!(document.content(), &after_trees);
    assert_eq!(document.content().grid().get(2, 2), Some(CombatTile::Tree));
    assert_eq!(document.content().grid().get(4, 4), Some(CombatTile::Tree));
    assert_eq!(document.content().grid().get(5, 5), Some(CombatTile::Grass));
    assert_eq!(document.undo_depth(), 1);
    assert!(document.is_dirty());

    assert_eq!(document.redo(), Ok(true));
    assert_eq!(document.content().grid().get(3, 3), Some(CombatTile::ShallowWater));
    assert_eq!(document.content().grid().get(6, 6), Some(CombatTile::ShallowWater));
}

// A stroke that authors nothing must not become an undo entry, and a cell off the map is
// refused with the same values a dungeon stroke gets.
#[test]
fn a_stroke_that_changes_nothing_or_leaves_the_map_is_refused() {
    let mut document = Document::new(map());
    let grass = CombatEdit::PaintTiles {
        changes: vec![TileChange { x: 1, y: 1, tile: CombatTile::Grass }],
    };
    assert_eq!(document.apply(grass), Err(EditError::EmptyPaint));
    assert_eq!(document.undo_depth(), 0);
    assert!(!document.is_dirty());

    let outside = CombatEdit::PaintTiles {
        changes: vec![TileChange { x: 30, y: 0, tile: CombatTile::Mud }],
    };
    assert!(matches!(document.apply(outside), Err(EditError::CellOutOfGrid { x: 30, .. })));
    assert_eq!(document.content(), &map());
}

// The room brush reads its wall and floor off the vocabulary, so on a combat map it must
// lay combat walls and floors rather than anything of the dungeon's.
#[test]
fn the_room_brush_lays_combat_walls_around_combat_floor() {
    let mut map = map();
    let room = stroke(&map, Brush::Room, (1, 1), (4, 4), CombatTile::Tree);
    room.apply(&mut map).unwrap();
    assert_eq!(map.grid().get(1, 1), Some(CombatTile::Wall));
    assert_eq!(map.grid().get(4, 2), Some(CombatTile::Wall));
    assert_eq!(map.grid().get(2, 2), Some(CombatTile::Floor));
    assert_eq!(map.grid().get(3, 3), Some(CombatTile::Floor));
    assert_eq!(map.grid().get(5, 5), Some(CombatTile::Grass));
}

// A combat map is measured in feet as a dungeon is, and a pace in days means nothing on it.
#[test]
fn a_combat_cell_is_five_feet_and_never_travelled() {
    let worth = worth_of_grid(map().grid());
    assert_eq!(worth.known_unit(), Some(DistanceUnit::Feet));
    assert!((worth.units_per_cell() - 5.0).abs() < 1e-6);
    assert!(!worth.travelled());
}

// The variant name is what a saved map will write, so it must survive the round trip.
#[test]
fn a_combat_tile_round_trips_through_its_name() {
    for tile in CombatTile::all() {
        let written = ron::to_string(&tile).unwrap();
        assert_eq!(ron::from_str::<CombatTile>(&written).unwrap(), tile, "{written}");
    }
    assert_eq!(ron::to_string(&CombatTile::ShallowWater).unwrap(), "ShallowWater");
}

// Two open maps must never share a name, since the panel and the socket both find a map
// by it.
#[test]
fn a_taken_name_gets_the_next_free_number() {
    assert_eq!(unused_name(" Ford ", ["Bridge"]), "Ford");
    assert_eq!(unused_name("Ford", ["Ford"]), "Ford 2");
    assert_eq!(unused_name("Ford", ["Ford", "Ford 2"]), "Ford 3");
}
