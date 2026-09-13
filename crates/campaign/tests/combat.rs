//! The combat map as a document: the strip order its vocabulary fixes, what a new map is
//! filled with, what it refuses, that a stroke is exactly one press of undo, and what a
//! stored map is on disk.

use campaign::brush::{self, Brush, TileChange};
use campaign::combat::{
    CombatEdit, CombatMap, CombatProblem, DEFAULT_COMBAT_CELLS, file_name_refusal, stem_of, stored, unused_name,
};
use campaign::document::Document;
use campaign::edit::EditError;
use campaign::grid::{DEFAULT_METRES_PER_CELL, GridProblem, TileGrid, TileVocabulary};
use campaign::layout;
use campaign::measure::{DistanceUnit, worth_of_grid};
use campaign::slug::combat_map_name;
use campaign::tiles::{COMBAT_TILE_COUNT, CombatTile, DungeonTile};
use campaign::world::{World, WorldError};

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

fn painted(width: u32, height: u32) -> CombatMap {
    let mut map = CombatMap::new("Bridge", width, height).unwrap();
    stroke(&map, Brush::Rectangle, (1, 1), (3, 3), CombatTile::Tree).apply(&mut map).unwrap();
    stroke(&map, Brush::Rectangle, (0, height as i64 - 1), (0, height as i64 - 1), CombatTile::DeepWater)
        .apply(&mut map)
        .unwrap();
    map
}

// The first acceptance criterion: a map saved and opened again is the same size with the
// same tiles, named by its file.
#[test]
fn a_saved_map_loads_as_the_same_grid() {
    let root = tempfile::tempdir().unwrap();
    let map = painted(12, 7);
    let path = layout::combat_map(root.path(), "bridge.ron");
    map.save(&path).unwrap();

    let loaded = CombatMap::load(&path, "bridge").unwrap();
    assert_eq!(loaded.grid(), map.grid());
    assert_eq!((loaded.grid().width(), loaded.grid().height()), (12, 7));
    assert_eq!(loaded.grid().get(2, 2), Some(CombatTile::Tree));
    assert_eq!(loaded.grid().get(0, 6), Some(CombatTile::DeepWater));
    assert_eq!(loaded.name(), "bridge");
}

// The second acceptance criterion and the clone case: a campaign with no `combat/` gains
// the directory and exactly one file in it, holding nothing but the grid.
#[test]
fn saving_into_a_campaign_without_combat_creates_one_file() {
    let root = tempfile::tempdir().unwrap();
    painted(8, 8).save(&layout::combat_map(root.path(), "bridge.ron")).unwrap();

    let top: Vec<String> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(top, ["combat"]);
    assert_eq!(stored(root.path()).unwrap(), ["bridge.ron"]);

    let text = std::fs::read_to_string(layout::combat_map(root.path(), "bridge.ron")).unwrap();
    assert!(text.contains("runs:") && text.contains("width: 8"), "{text}");
    assert!(!text.contains("Bridge"), "the name is not part of the file: {text}");
}

// The third acceptance criterion: two maps typed with one name are given two files, each
// a name the file-name rule accepts.
#[test]
fn a_taken_file_name_gets_the_next_free_number() {
    let first = combat_map_name("Bridge", []);
    let second = combat_map_name("Bridge", [first.as_str()]);
    let third = combat_map_name("  bridge!  ", [first.as_str(), second.as_str()]);
    assert_eq!([first.as_str(), second.as_str(), third.as_str()], ["bridge.ron", "bridge-2.ron", "bridge-3.ron"]);
    for name in [&first, &second, &third] {
        assert_eq!(file_name_refusal(name), None, "{name}");
    }
    assert_eq!(stem_of(&second), "bridge-2");
}

// The panel lists the directory, so only stored maps may appear in it: not a save's
// temporary, not another kind of file, not a directory, and nothing at all before the
// first save.
#[test]
fn the_stored_list_holds_only_combat_map_files() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(stored(root.path()).unwrap(), Vec::<String>::new());

    let combat = layout::combat(root.path());
    std::fs::create_dir_all(combat.join("nested.ron")).unwrap();
    for name in ["bridge-2.ron", "bridge.ron", "bridge.ron.123.tmp", "notes.txt", ".hidden.ron"] {
        std::fs::write(combat.join(name), "").unwrap();
    }
    assert_eq!(stored(root.path()).unwrap(), ["bridge-2.ron", "bridge.ron"]);
}

// The fourth acceptance criterion: a hand-edited file whose runs do not cover its extent
// is refused with the very sentence a dungeon with the same grid gets.
#[test]
fn a_tile_count_that_does_not_match_is_refused_as_a_dungeons_is() {
    let path = std::path::Path::new("/campaign/combat/bridge.ron");
    let combat_text = CombatMap::new("Bridge", 8, 8).unwrap().to_ron().unwrap();
    let dungeon_text = World::on_a_grid(TileGrid::<DungeonTile>::new(8, 8, DEFAULT_METRES_PER_CELL).unwrap())
        .to_ron()
        .unwrap();
    assert_eq!(combat_text.matches("64").count(), 1, "{combat_text}");
    assert_eq!(dungeon_text.matches("64").count(), 1, "{dungeon_text}");

    let combat = CombatMap::from_ron(&combat_text.replace("64", "63"), path, "bridge").unwrap_err();
    let dungeon = World::from_ron(&dungeon_text.replace("64", "63"), path).unwrap_err();
    let mismatch = GridProblem::TileCountMismatch {
        width: 8,
        height: 8,
        want: 64,
        have: 63,
    };
    assert!(matches!(&combat, WorldError::BadGrid { source, .. } if *source == mismatch), "{combat}");
    assert_eq!(combat.to_string(), dungeon.to_string());
}

// A file that is not RON, or not there, must say so rather than open as a blank map.
#[test]
fn an_unparseable_or_absent_file_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let path = layout::combat_map(root.path(), "bridge.ron");
    assert!(matches!(CombatMap::load(&path, "bridge"), Err(WorldError::WorldUnreadable { .. })));

    std::fs::create_dir_all(layout::combat(root.path())).unwrap();
    std::fs::write(&path, "not a grid").unwrap();
    assert!(matches!(CombatMap::load(&path, "bridge"), Err(WorldError::WorldMalformed { .. })));
}

// The close guard trusts the dirty flag, so a save that failed must leave the map unsaved
// and one that landed must not.
#[test]
fn a_combat_document_is_clean_only_once_its_save_lands() {
    let root = tempfile::tempdir().unwrap();
    let mut document = Document::new(map());
    let trees = stroke(document.content(), Brush::Rectangle, (2, 2), (4, 4), CombatTile::Tree);
    document.apply(trees).unwrap();

    let blocker = root.path().join("combat");
    std::fs::write(&blocker, "a file where the directory would go").unwrap();
    assert!(document.save(&blocker.join("bridge.ron")).is_err());
    assert!(document.is_dirty());

    document.save(&root.path().join("elsewhere").join("bridge.ron")).unwrap();
    assert!(!document.is_dirty());
    assert_eq!(document.undo_depth(), 1);
}
