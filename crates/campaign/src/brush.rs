//! The ways a stroke turns a grid and a gesture into the cells it would change.
//!
//! Every brush answers one question — which cells, and what do they become — and none of
//! them touches a grid. What comes back is the list an
//! [`Edit::PaintTiles`](crate::edit::Edit::PaintTiles) carries, which is what makes a
//! stroke one entry on the undo stack however many cells it covers.
//!
//! The list is always filtered: a cell outside the grid, a cell already carrying the tile
//! it would be given, and a cell named twice all come out. That is what makes an empty
//! answer mean "this stroke changes nothing" rather than "this stroke was refused", and it
//! is why the inverse of a paint is exact without depending on the order it is applied in.

use serde::{Deserialize, Serialize};

use crate::grid::TileGrid;
use crate::tiles::DungeonTile;

/// What a stroke does to the cells it covers.
///
/// Deliberately not `#[non_exhaustive]`: a consumer matching on a brush to label it or
/// to decide what a gesture means should fail to compile when one is added.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Brush {
    /// Every cell the pointer passed over.
    #[default]
    Freehand,
    /// Every cell between the two corners.
    Rectangle,
    /// The run of cells reachable from the first that carry the tile it does.
    Flood,
    /// A wall border on the rectangle's edge, floor inside it.
    Room,
}

impl Brush {
    /// Every brush, in the order the strip offers them.
    ///
    /// Written out rather than derived, and the array length is the count.
    pub const fn all() -> [Self; 4] {
        [Self::Freehand, Self::Rectangle, Self::Flood, Self::Room]
    }

    /// The word for this brush in the tool strip.
    pub fn label(self) -> &'static str {
        match self {
            Self::Freehand => "paint",
            Self::Rectangle => "rect",
            Self::Flood => "fill",
            Self::Room => "room",
        }
    }

    /// Whether this brush is decided by the whole path or only by where it started and
    /// ended.
    ///
    /// The one thing a caller needs in order to know whether to keep sampling: a
    /// freehand stroke has to record every cell as it goes, and the other three need
    /// only the two corners.
    pub fn tracks_the_path(self) -> bool {
        matches!(self, Self::Freehand)
    }
}

/// One cell becoming one tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileChange {
    pub x: u32,
    pub y: u32,
    pub tile: DungeonTile,
}

/// The cells `brush` would change, given the path the gesture traced and the tile in
/// hand.
///
/// `path` is every cell the gesture covered, in order. [`Brush::Freehand`] uses all of
/// it; the others use only its first and last cell. [`Brush::Room`] ignores `tile`
/// entirely — a room is floor inside a wall border, which is the whole point of it being
/// its own brush rather than two rectangles.
///
/// Empty for a path that changes nothing, which is not a failure: a click on a cell that
/// already carries the tile, a flood fill whose region is already that tile, and an empty
/// path all come back empty, and none of them should become an edit.
///
/// Every cell in the result is inside `grid`, carries a tile it does not already have, and
/// appears exactly once.
pub fn cells(
    grid: &TileGrid,
    brush: Brush,
    path: &[(i64, i64)],
    tile: DungeonTile,
) -> Vec<TileChange> {
    let (Some(&first), Some(&last)) = (path.first(), path.last()) else {
        return Vec::new();
    };

    let painted: Vec<((i64, i64), DungeonTile)> = match brush {
        Brush::Freehand => path.iter().map(|cell| (*cell, tile)).collect(),
        Brush::Rectangle => rectangle(first, last).map(|cell| (cell, tile)).collect(),
        Brush::Flood => flood(grid, first, tile).into_iter().map(|cell| (cell, tile)).collect(),
        Brush::Room => room(first, last).collect(),
    };

    keep_the_changes(grid, painted)
}

/// Every cell a straight line from `from` to `to` passes through.
///
/// A freehand stroke is sampled once a frame, and at sixty frames a second a normal
/// sweep of the mouse crosses several cells between samples — so without this the brush
/// paints a dotted line. Bresenham, and it always includes both ends.
pub fn line(from: (i64, i64), to: (i64, i64)) -> Vec<(i64, i64)> {
    let (mut x, mut y) = from;
    let (tx, ty) = to;

    let dx = (tx - x).abs();
    let dy = -(ty - y).abs();
    let step_x = if x < tx { 1 } else { -1 };
    let step_y = if y < ty { 1 } else { -1 };
    let mut error = dx + dy;

    let mut cells = vec![(x, y)];
    while (x, y) != (tx, ty) {
        let doubled = error * 2;
        if doubled >= dy {
            error += dy;
            x += step_x;
        }
        if doubled <= dx {
            error += dx;
            y += step_y;
        }
        cells.push((x, y));
    }
    cells
}

fn rectangle(from: (i64, i64), to: (i64, i64)) -> impl Iterator<Item = (i64, i64)> {
    let (low_x, high_x) = (from.0.min(to.0), from.0.max(to.0));
    let (low_y, high_y) = (from.1.min(to.1), from.1.max(to.1));
    (low_y..=high_y).flat_map(move |y| (low_x..=high_x).map(move |x| (x, y)))
}

fn room(from: (i64, i64), to: (i64, i64)) -> impl Iterator<Item = ((i64, i64), DungeonTile)> {
    let (low_x, high_x) = (from.0.min(to.0), from.0.max(to.0));
    let (low_y, high_y) = (from.1.min(to.1), from.1.max(to.1));
    rectangle(from, to).map(move |(x, y)| {
        let border = x == low_x || x == high_x || y == low_y || y == high_y;
        let tile = if border {
            DungeonTile::Wall
        } else {
            DungeonTile::Floor
        };
        ((x, y), tile)
    })
}

fn flood(grid: &TileGrid, start: (i64, i64), tile: DungeonTile) -> Vec<(i64, i64)> {
    let Some(want) = grid.get(start.0, start.1) else {
        return Vec::new();
    };
    if want == tile {
        return Vec::new();
    }

    let width = i64::from(grid.width());
    let index = |x: i64, y: i64| (y * width + x) as usize;

    let mut seen = vec![false; grid.tiles().len()];
    let mut stack = vec![start];
    let mut found = Vec::new();

    seen[index(start.0, start.1)] = true;
    while let Some((x, y)) = stack.pop() {
        found.push((x, y));
        for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            if grid.get(nx, ny) != Some(want) {
                continue;
            }
            let slot = index(nx, ny);
            if !seen[slot] {
                seen[slot] = true;
                stack.push((nx, ny));
            }
        }
    }
    found
}

fn keep_the_changes(
    grid: &TileGrid,
    painted: Vec<((i64, i64), DungeonTile)>,
) -> Vec<TileChange> {
    let width = i64::from(grid.width());
    let mut wanted: Vec<Option<DungeonTile>> = vec![None; grid.tiles().len()];
    let mut order: Vec<usize> = Vec::new();

    for ((x, y), tile) in painted {
        if grid.get(x, y) == Some(tile) || !grid.holds(x, y) {
            continue;
        }
        let slot = (y * width + x) as usize;
        if wanted[slot].is_none() {
            order.push(slot);
        }
        wanted[slot] = Some(tile);
    }

    order
        .into_iter()
        .filter_map(|slot| {
            let tile = wanted[slot]?;
            let width = width as usize;
            Some(TileChange {
                x: (slot % width) as u32,
                y: (slot / width) as u32,
                tile,
            })
        })
        .collect()
}
