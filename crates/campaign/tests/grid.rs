//! The rules a tile grid must satisfy, what it refuses, and the two properties a document
//! carrying one rests on: a cell outside the grid is never mistaken for one inside it, and
//! a grid survives the round trip through its own file format.

use campaign::grid::{GridProblem, MAX_GRID_CELLS, TileGrid};
use campaign::tiles::{DungeonTile, GRID_COARSE_CELLS, grid_chunk_tiles, grid_lines};
use campaign::world::World;

fn grid(width: u32, height: u32) -> TileGrid {
    TileGrid::new(width, height, 1.5).expect("the fixture must be a legal grid")
}

// A grid nothing could be painted on is refused where it is built, because there is no
// edit that resizes one — so a zero dimension would be permanent.
#[test]
fn a_grid_with_no_extent_is_refused() {
    for (width, height) in [(0, 8), (8, 0), (0, 0)] {
        assert!(matches!(
            TileGrid::new(width, height, 1.5),
            Err(GridProblem::NoExtent { .. })
        ));
    }
}

// The scale divides the camera's bounds and the cursor-to-cell conversion, so one that
// cannot be compared would break both for the life of the session with nothing to say why.
#[test]
fn a_scale_that_is_not_a_finite_positive_number_is_refused() {
    for scale in [f32::NAN, f32::INFINITY, 0.0, -1.5] {
        assert!(
            matches!(
                TileGrid::new(8, 8, scale),
                Err(GridProblem::BadScale { .. })
            ),
            "a scale of {scale} was accepted"
        );
    }
}

// The limit exists so the refusal reaches the GM at creation rather than at the first
// save, and it is computed in u64 because the u32 product of two plausible-looking
// dimensions wraps.
#[test]
fn a_grid_over_the_cell_limit_is_refused_without_overflowing() {
    assert!(matches!(
        TileGrid::new(4_000_000_000, 4_000_000_000, 1.5),
        Err(GridProblem::TooManyCells { .. })
    ));
    let side = (MAX_GRID_CELLS as f64).sqrt() as u32 + 1;
    assert!(matches!(
        TileGrid::new(side, side, 1.5),
        Err(GridProblem::TooManyCells { .. })
    ));
}

// Tiles that do not cover the extent would let an index land on a cell that is not there.
#[test]
fn tiles_that_do_not_cover_the_extent_are_refused() {
    let problem = TileGrid::from_tiles(4, 4, 1.5, vec![DungeonTile::Floor; 15]);
    assert!(matches!(
        problem,
        Err(GridProblem::TileCountMismatch {
            want: 16,
            have: 15,
            ..
        })
    ));
}

// The whole reason `get` takes signed coordinates and bounds each axis on its own: chunk
// coordinates are signed because the camera pans past the origin, and flattening
// `y * width + x` first turns (-1, 1) into an in-bounds cell on the row above — so the
// grid's edge would wrap instead of ending.
#[test]
fn a_negative_coordinate_does_not_wrap_onto_the_previous_row() {
    let mut grid = grid(8, 8);
    let painted = campaign::brush::cells(
        &grid,
        campaign::brush::Brush::Freehand,
        &[(7, 0)],
        DungeonTile::Wall,
    );
    assert_eq!(painted.len(), 1);

    let mut world = World::on_a_grid(grid.clone());
    campaign::edit::Edit::PaintTiles { changes: painted }
        .apply(&mut world)
        .expect("painting one in-bounds cell must land");
    grid = world.grid().expect("the grid is still there").clone();

    assert_eq!(grid.get(7, 0), Some(DungeonTile::Wall));
    assert_eq!(grid.get(-1, 1), None, "(-1, 1) must not resolve to (7, 0)");
    assert_eq!(grid.get(8, 0), None);
    assert_eq!(grid.get(0, -1), None);
    assert_eq!(grid.get(0, 8), None);
    assert!(!grid.holds(-1, 1));
}

// A grid is written run-length encoded, so the round trip is the only thing standing
// between a painted dungeon and a file that reads back as a different one.
#[test]
fn a_painted_grid_survives_the_round_trip_through_ron() {
    let mut tiles = vec![DungeonTile::Empty; 64];
    for (index, tile) in tiles.iter_mut().enumerate() {
        *tile = match index % 7 {
            0 => DungeonTile::Floor,
            1 => DungeonTile::Wall,
            2 => DungeonTile::Door,
            3 => DungeonTile::SecretDoor,
            4 => DungeonTile::StairsUp,
            5 => DungeonTile::Water,
            _ => DungeonTile::Empty,
        };
    }
    let grid = TileGrid::from_tiles(8, 8, 1.5, tiles).expect("a legal grid");
    let world = World::on_a_grid(grid);

    let text = world.to_ron().expect("a world must serialize");
    let back = World::from_ron(&text, std::path::Path::new("dungeon.ron"))
        .expect("what this build wrote, it must read");
    assert_eq!(back, world);
}

// The encoding exists to keep a painted dungeon inside MAX_WORLD_BYTES. A grid of one
// repeated tile is the case it is built for, and it must not cost per cell.
#[test]
fn an_unpainted_grid_costs_far_less_than_a_variant_name_per_cell() {
    let world = World::on_a_grid(grid(256, 256));
    let text = world.to_ron().expect("a world must serialize");
    assert!(
        text.len() < 4_000,
        "65536 empty cells serialized to {} bytes",
        text.len()
    );
}

// A document that parses but carries an incoherent grid must be refused, or every
// consumer downstream has to check what the format already promised.
#[test]
fn a_document_carrying_an_incoherent_grid_is_refused() {
    let text = r#"(
    version: 1,
    next_id: 0,
    features: {},
    grid: Some((
        width: 4,
        height: 4,
        metres_per_cell: 1.5,
        runs: [(3, Floor)],
    )),
)"#;
    let refusal = World::from_ron(text, std::path::Path::new("dungeon.ron"));
    assert!(
        matches!(refusal, Err(campaign::world::WorldError::BadGrid { .. })),
        "got {refusal:?}"
    );
}

// A run count from a hostile file must not be asked for as an allocation length.
#[test]
fn a_hostile_run_count_is_bounded_rather_than_allocated() {
    let text = r#"(
    version: 1,
    next_id: 0,
    features: {},
    grid: Some((
        width: 4,
        height: 4,
        metres_per_cell: 1.5,
        runs: [(4294967295, Floor)],
    )),
)"#;
    let refusal = World::from_ron(text, std::path::Path::new("dungeon.ron"));
    assert!(
        matches!(refusal, Err(campaign::world::WorldError::BadGrid { .. })),
        "got {refusal:?}"
    );
}

// A world map carries no grid, and that is what decides which backdrop it is drawn on.
#[test]
fn a_world_without_a_grid_round_trips_without_gaining_one() {
    let world = World::default();
    let text = world.to_ron().expect("a world must serialize");
    assert!(!text.contains("grid"), "an absent grid must not be written");
    let back =
        World::from_ron(&text, std::path::Path::new("world.ron")).expect("an empty world reads");
    assert!(back.grid().is_none());
}

// A chunk overhanging the grid must have a ragged edge rather than a wrapped one, and
// "outside the grid" must stay a different answer from "unexcavated rock".
#[test]
fn a_chunk_off_the_grid_comes_back_undrawn_rather_than_empty() {
    let grid = grid(4, 4);
    let off = grid_chunk_tiles(&grid, 5, 5);
    assert!(off.iter().all(Option::is_none));

    let first = grid_chunk_tiles(&grid, 0, 0);
    let drawn = first.iter().filter(|tile| tile.is_some()).count();
    assert_eq!(drawn, 16, "only the grid's own cells are drawn");
    assert!(
        first.iter().flatten().all(|tile| *tile == DungeonTile::Empty),
        "a cell inside the grid is Empty, which is a tile"
    );
}

// A terrain's rows run top to bottom and a chunk's run bottom to top; the flip has to
// happen for a grid exactly as it does for a terrain, or north is wrong in a dungeon.
#[test]
fn a_grid_chunks_rows_are_flipped_like_a_terrains() {
    let mut tiles = vec![DungeonTile::Empty; 4];
    tiles[0] = DungeonTile::Wall;
    let grid = TileGrid::from_tiles(2, 2, 1.5, tiles).expect("a legal grid");

    let chunk = grid_chunk_tiles(&grid, 0, 0);
    let side = campaign::tiles::CHUNK_CELLS as usize;
    assert_eq!(
        chunk[(side - 1) * side],
        Some(DungeonTile::Wall),
        "the grid's first row must land on the chunk's top row"
    );
}

// The grid is a ruler. Two lines closer together than a few pixels are a wash of colour,
// and a level that appeared at full strength the frame another vanished would be the most
// visible kind of flicker there is — so both levels fade, independently.
#[test]
fn both_grid_levels_fade_rather_than_stepping() {
    let close = grid_lines(0.02);
    assert_eq!(close.fine, 1.0, "every cell is ruled when a cell is large");
    assert_eq!(close.coarse, 1.0);

    let far = grid_lines(50.0);
    assert!(far.draws_nothing(), "a grid that dense is not drawn at all");

    assert!(grid_lines(0.0).draws_nothing());
    assert!(grid_lines(f32::NAN).draws_nothing());
    assert!(grid_lines(-1.0).draws_nothing());

    let mut saw_coarse_alone = false;
    let mut previous = grid_lines(0.01);
    for step in 1..2000 {
        let cells_per_pixel = 0.01 * f32::powf(1.005, step as f32);
        let now = grid_lines(cells_per_pixel);
        assert!(
            now.coarse >= now.fine - f32::EPSILON,
            "the fine level outlived the coarse one at {cells_per_pixel}"
        );
        assert!(
            (now.fine - previous.fine).abs() < 0.1 && (now.coarse - previous.coarse).abs() < 0.1,
            "the grid jumped at {cells_per_pixel}: {previous:?} to {now:?}"
        );
        if now.coarse > 0.0 && now.fine == 0.0 {
            saw_coarse_alone = true;
        }
        previous = now;
    }
    assert!(
        saw_coarse_alone,
        "there must be a range where only the coarse level is ruled"
    );
    assert!(previous.draws_nothing(), "the grid ends up drawn not at all");
}

// The coarse level steps by the major line a battle map is ruled in, so it reads as a
// scale rather than as a degraded fine grid.
#[test]
fn the_coarse_level_steps_by_the_conventional_major_line() {
    assert_eq!(GRID_COARSE_CELLS, 5);
}
