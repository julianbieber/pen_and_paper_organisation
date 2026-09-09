//! What each brush covers, and the one filter everything else rests on: a stroke never
//! carries a cell it would not change, so an empty answer means the gesture authored
//! nothing rather than that it was refused.

use campaign::brush::{self, Brush, TileChange};
use campaign::grid::TileGrid;
use campaign::tiles::DungeonTile;

fn grid() -> TileGrid {
    TileGrid::new(8, 8, 1.5).expect("the fixture must be a legal grid")
}

fn painted(grid: &TileGrid, changes: &[TileChange]) -> TileGrid {
    let mut tiles = grid.tiles().to_vec();
    for change in changes {
        tiles[(change.y * grid.width() + change.x) as usize] = change.tile;
    }
    TileGrid::from_tiles(grid.width(), grid.height(), grid.metres_per_cell(), tiles)
        .expect("painting cells cannot change the extent")
}

fn cells_of(changes: &[TileChange]) -> Vec<(u32, u32)> {
    changes.iter().map(|change| (change.x, change.y)).collect()
}

// Freehand paints exactly the cells the gesture traced, and nothing else.
#[test]
fn freehand_paints_the_path_it_was_given() {
    let grid = grid();
    let changes = brush::cells(
        &grid,
        Brush::Freehand,
        &[(1, 1), (2, 1), (3, 1)],
        DungeonTile::Floor,
    );
    assert_eq!(cells_of(&changes), vec![(1, 1), (2, 1), (3, 1)]);
    assert!(changes.iter().all(|c| c.tile == DungeonTile::Floor));
}

// A stroke is sampled once a frame, so without interpolation a normal sweep of the mouse
// draws a dotted line. The line has to be unbroken and has to include both ends.
#[test]
fn a_line_between_two_sampled_cells_is_unbroken() {
    let cells = brush::line((0, 0), (5, 3));
    assert_eq!(cells.first(), Some(&(0, 0)));
    assert_eq!(cells.last(), Some(&(5, 3)));
    for pair in cells.windows(2) {
        let step = ((pair[1].0 - pair[0].0).abs(), (pair[1].1 - pair[0].1).abs());
        assert!(step.0 <= 1 && step.1 <= 1, "the line jumped by {step:?}");
    }
    assert_eq!(brush::line((2, 2), (2, 2)), vec![(2, 2)]);
}

// A rectangle covers both corners whichever order they were dragged in.
#[test]
fn a_rectangle_covers_its_corners_in_either_direction() {
    let grid = grid();
    let forwards = brush::cells(&grid, Brush::Rectangle, &[(1, 1), (3, 2)], DungeonTile::Floor);
    let backwards = brush::cells(&grid, Brush::Rectangle, &[(3, 2), (1, 1)], DungeonTile::Floor);
    assert_eq!(forwards.len(), 6);
    let mut a = cells_of(&forwards);
    let mut b = cells_of(&backwards);
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b);
}

// The room stamp is the operation actually used most: floor inside a wall border, in one
// gesture and one undo step. It ignores the tile in hand, which is why it is its own brush.
#[test]
fn a_room_lays_floor_inside_a_wall_border() {
    let grid = grid();
    let changes = brush::cells(&grid, Brush::Room, &[(1, 1), (4, 4)], DungeonTile::Water);
    assert_eq!(changes.len(), 16);

    let after = painted(&grid, &changes);
    assert_eq!(after.get(1, 1), Some(DungeonTile::Wall));
    assert_eq!(after.get(4, 1), Some(DungeonTile::Wall));
    assert_eq!(after.get(1, 4), Some(DungeonTile::Wall));
    assert_eq!(after.get(2, 2), Some(DungeonTile::Floor));
    assert_eq!(after.get(3, 3), Some(DungeonTile::Floor));
    assert!(
        !changes.iter().any(|c| c.tile == DungeonTile::Water),
        "a room ignores the tile in hand"
    );
}

// A one-cell-wide rectangle is all border, so it is a wall rather than a room with an
// inside that does not exist.
#[test]
fn a_room_one_cell_wide_is_all_wall() {
    let grid = grid();
    let changes = brush::cells(&grid, Brush::Room, &[(2, 1), (2, 5)], DungeonTile::Floor);
    assert_eq!(changes.len(), 5);
    assert!(changes.iter().all(|c| c.tile == DungeonTile::Wall));
}

// A fill spreads over cells carrying what the start cell carries, stops at anything else,
// and is 4-connected so it does not leak through a diagonal gap in a wall.
#[test]
fn a_fill_stops_at_a_wall_and_does_not_leak_diagonally() {
    let grid = grid();
    let walls = brush::cells(&grid, Brush::Room, &[(1, 1), (4, 4)], DungeonTile::Floor);
    let grid = painted(&grid, &walls);

    let filled = brush::cells(&grid, Brush::Flood, &[(2, 2)], DungeonTile::Water);
    assert_eq!(filled.len(), 4, "only the room's interior is reached");
    let mut inside = cells_of(&filled);
    inside.sort_unstable();
    assert_eq!(inside, vec![(2, 2), (2, 3), (3, 2), (3, 3)]);
}

// A fill of a region that already carries the target tile must yield nothing rather than
// walking the whole region to produce a list that is then thrown away.
#[test]
fn a_fill_onto_the_tile_already_there_yields_nothing() {
    let grid = grid();
    assert!(brush::cells(&grid, Brush::Flood, &[(0, 0)], DungeonTile::Empty).is_empty());
}

// The filter is what makes "one stroke that changes nothing is never an edit" true, and it
// is what makes a paint's inverse exact without depending on the order it is applied in.
#[test]
fn a_stroke_never_carries_a_cell_it_would_not_change() {
    let grid = grid();
    let floor = brush::cells(&grid, Brush::Rectangle, &[(0, 0), (3, 3)], DungeonTile::Floor);
    let grid = painted(&grid, &floor);

    let again = brush::cells(&grid, Brush::Rectangle, &[(0, 0), (3, 3)], DungeonTile::Floor);
    assert!(again.is_empty(), "repainting the same tile changes nothing");

    let overlap = brush::cells(&grid, Brush::Rectangle, &[(2, 2), (5, 5)], DungeonTile::Floor);
    assert!(
        overlap.iter().all(|c| c.x > 3 || c.y > 3),
        "only the cells that were not already floor come back"
    );
}

// A stroke that crosses its own path names a cell twice; keeping the last word is what
// makes the inverse exact.
#[test]
fn a_stroke_that_crosses_itself_names_each_cell_once() {
    let grid = grid();
    let changes = brush::cells(
        &grid,
        Brush::Freehand,
        &[(1, 1), (2, 1), (1, 1), (2, 1)],
        DungeonTile::Floor,
    );
    assert_eq!(changes.len(), 2);
    let mut seen = cells_of(&changes);
    seen.sort_unstable();
    assert_eq!(seen, vec![(1, 1), (2, 1)]);
}

// A gesture that ran off the edge of the grid paints what it covered and drops the rest,
// rather than being refused wholesale or wrapping onto the far side.
#[test]
fn cells_outside_the_grid_are_dropped_rather_than_wrapped() {
    let grid = grid();
    let changes = brush::cells(
        &grid,
        Brush::Freehand,
        &[(-2, 0), (-1, 0), (0, 0), (1, 0), (99, 0), (0, -1)],
        DungeonTile::Floor,
    );
    assert_eq!(cells_of(&changes), vec![(0, 0), (1, 0)]);
}

// An empty gesture is not a failure and not an edit.
#[test]
fn an_empty_path_changes_nothing() {
    let grid = grid();
    for brush in Brush::all() {
        assert!(brush::cells(&grid, brush, &[], DungeonTile::Floor).is_empty());
    }
}
