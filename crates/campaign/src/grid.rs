//! The square tile grid a document may be drawn on, and every reason one is refused.
//!
//! A grid is a backdrop the GM authored rather than one imported from a terrain, so it
//! is part of the document that holds it and travels with it. Changing one is not this
//! module's business — [`crate::edit`] owns that, through the single crate-visible
//! accessor that exists for it and for nothing else.
//!
//! Tiles are stored flat and written run-length encoded. A hand-painted dungeon is long
//! runs of the same tile, and one variant name per cell would put a middling grid over
//! [`MAX_WORLD_BYTES`](crate::world::MAX_WORLD_BYTES) — a document that paints happily
//! and can never be saved.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::tiles::DungeonTile;

/// Cells along one side of a grid a dungeon is created with.
pub const DEFAULT_GRID_CELLS: u32 = 64;

/// Metres one cell spans in a grid a dungeon is created with.
///
/// A five-foot square, which is what the room descriptions this tool is pointed at are
/// written in.
pub const DEFAULT_METRES_PER_CELL: f32 = 1.524;

/// The most cells a grid may cover.
///
/// Enforced where a grid is created and where a document carrying one is read, so the
/// refusal reaches the GM before the painting rather than at the first save. It is well
/// under what [`MAX_WORLD_BYTES`](crate::world::MAX_WORLD_BYTES) admits once the tiles
/// are run-length encoded, so the two limits cannot disagree about a grid that was
/// painted rather than adversarially built.
pub const MAX_GRID_CELLS: u64 = 1 << 20;

/// A grid a document may not hold.
///
/// Its own type for the reason [`ParentProblem`](crate::world::ParentProblem) is: the
/// same mistakes are reachable both by reading a document and by applying an edit to one
/// already open, and a reader should get the same sentence either way.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GridProblem {
    /// A grid with no cells in one direction. There is no edit that resizes a grid, so
    /// this is a grid nothing could ever be painted on.
    #[error("a grid of {width}x{height} has no extent, and nothing could be painted on it")]
    NoExtent { width: u32, height: u32 },

    /// A scale that is not a finite positive number.
    ///
    /// Refused for the reason a non-finite reveal threshold is: the scale divides the
    /// camera's bounds and the cursor-to-cell conversion, and one that cannot be
    /// compared would break both for the life of the session with nothing to say why.
    #[error("a grid scale of {metres_per_cell} metres per cell {reason}")]
    BadScale {
        metres_per_cell: f32,
        reason: &'static str,
    },

    /// The tiles do not cover the extent exactly.
    #[error("a grid of {width}x{height} covers {want} cells, and {have} tiles were given")]
    TileCountMismatch {
        width: u32,
        height: u32,
        want: u64,
        have: u64,
    },

    /// A grid over [`MAX_GRID_CELLS`].
    #[error("a grid of {cells} cells is over the {limit} cell limit")]
    TooManyCells { cells: u64, limit: u64 },
}

/// A square tile grid: how far it goes, what one cell is worth, and the tile under every
/// cell.
///
/// Every field is private and there is no way to change a tile but
/// [`Edit`](crate::edit::Edit), so a button and a control socket cannot come to mean
/// different things — the same rule [`World`](crate::world::World) holds its features
/// under.
///
/// A grid that exists is always coherent: it covers `width * height` cells, both are
/// non-zero, the product is within [`MAX_GRID_CELLS`], and the scale is a finite positive
/// number. [`TileGrid::new`] refuses anything else and
/// [`World::load`](crate::world::World::load) refuses a document carrying one that is
/// not, so nothing downstream has to check.
#[derive(Debug, Clone, PartialEq)]
pub struct TileGrid {
    width: u32,
    height: u32,
    metres_per_cell: f32,
    tiles: Vec<DungeonTile>,
}

impl TileGrid {
    /// A grid of `width` by `height` cells, every one of them
    /// [`DungeonTile::Empty`], at `metres_per_cell`.
    ///
    /// Fails [`GridProblem::NoExtent`] on a zero dimension,
    /// [`GridProblem::TooManyCells`] over [`MAX_GRID_CELLS`], and
    /// [`GridProblem::BadScale`] on a scale that is not a finite positive number.
    pub fn new(width: u32, height: u32, metres_per_cell: f32) -> Result<Self, GridProblem> {
        let cells = Self::check_extent(width, height, metres_per_cell)?;
        Ok(Self {
            width,
            height,
            metres_per_cell,
            tiles: vec![DungeonTile::Empty; cells as usize],
        })
    }

    /// A grid holding exactly `tiles`, in row-major order from the top-left.
    ///
    /// Fails as [`TileGrid::new`] does, and additionally
    /// [`GridProblem::TileCountMismatch`] when the tiles do not cover the extent.
    pub fn from_tiles(
        width: u32,
        height: u32,
        metres_per_cell: f32,
        tiles: Vec<DungeonTile>,
    ) -> Result<Self, GridProblem> {
        let cells = Self::check_extent(width, height, metres_per_cell)?;
        if tiles.len() as u64 != cells {
            return Err(GridProblem::TileCountMismatch {
                width,
                height,
                want: cells,
                have: tiles.len() as u64,
            });
        }
        Ok(Self {
            width,
            height,
            metres_per_cell,
            tiles,
        })
    }

    /// How far the grid goes across.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// How far the grid goes down.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// How many cells the grid covers.
    pub fn cells(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// What one cell is worth on the ground.
    pub fn metres_per_cell(&self) -> f32 {
        self.metres_per_cell
    }

    /// Every tile, row-major from the top-left.
    pub fn tiles(&self) -> &[DungeonTile] {
        &self.tiles
    }

    /// The tile at `x, y`, or `None` for a cell outside the grid.
    ///
    /// Signed and checked one axis at a time, which is the whole point: chunk
    /// coordinates are signed because the camera pans past the origin, and flattening
    /// `y * width + x` before bounding each axis turns `x = -1, y = 1` into an in-bounds
    /// cell on the row above — so the grid's edge would wrap instead of ending.
    pub fn get(&self, x: i64, y: i64) -> Option<DungeonTile> {
        self.tiles.get(self.flatten(x, y)?).copied()
    }

    /// Whether `x, y` names a cell this grid holds.
    pub fn holds(&self, x: i64, y: i64) -> bool {
        self.flatten(x, y).is_some()
    }

    /// Put `tile` at `x, y` and hand back what was there.
    ///
    /// `None` for a cell outside the grid, having changed nothing. Crate-visible: this
    /// exists for [`crate::edit`] and for nothing else.
    pub(crate) fn set(&mut self, x: i64, y: i64, tile: DungeonTile) -> Option<DungeonTile> {
        let index = self.flatten(x, y)?;
        Some(std::mem::replace(&mut self.tiles[index], tile))
    }

    /// Why this grid is not one a document may hold, or `None` if it is fine.
    ///
    /// The same question [`TileGrid::from_tiles`] asks, asked of a grid that arrived by
    /// another route — deserialization, which cannot fail into a `Result` here.
    pub fn refusal(&self) -> Option<GridProblem> {
        match Self::check_extent(self.width, self.height, self.metres_per_cell) {
            Err(problem) => Some(problem),
            Ok(cells) if self.tiles.len() as u64 != cells => Some(GridProblem::TileCountMismatch {
                width: self.width,
                height: self.height,
                want: cells,
                have: self.tiles.len() as u64,
            }),
            Ok(_) => None,
        }
    }

    fn flatten(&self, x: i64, y: i64) -> Option<usize> {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return None;
        }
        Some((y * i64::from(self.width) + x) as usize)
    }

    fn check_extent(width: u32, height: u32, metres_per_cell: f32) -> Result<u64, GridProblem> {
        if width == 0 || height == 0 {
            return Err(GridProblem::NoExtent { width, height });
        }
        if let Some(reason) = scale_refusal(metres_per_cell) {
            return Err(GridProblem::BadScale {
                metres_per_cell,
                reason,
            });
        }
        let cells = u64::from(width) * u64::from(height);
        if cells > MAX_GRID_CELLS {
            return Err(GridProblem::TooManyCells {
                cells,
                limit: MAX_GRID_CELLS,
            });
        }
        Ok(cells)
    }
}

/// Why `metres_per_cell` is not a scale a grid may carry, or `None` if it is fine.
pub fn scale_refusal(metres_per_cell: f32) -> Option<&'static str> {
    if !metres_per_cell.is_finite() {
        return Some("is not a finite number");
    }
    if metres_per_cell <= 0.0 {
        return Some("is not greater than zero");
    }
    None
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "TileGrid")]
struct GridOnDisk {
    width: u32,
    height: u32,
    metres_per_cell: f32,
    /// `(how many, which tile)`, in the order the flat tiles run.
    runs: Vec<(u32, DungeonTile)>,
}

impl Serialize for TileGrid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut runs: Vec<(u32, DungeonTile)> = Vec::new();
        for tile in &self.tiles {
            match runs.last_mut() {
                Some((count, last)) if last == tile && *count < u32::MAX => *count += 1,
                _ => runs.push((1, *tile)),
            }
        }
        GridOnDisk {
            width: self.width,
            height: self.height,
            metres_per_cell: self.metres_per_cell,
            runs,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TileGrid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let disk = GridOnDisk::deserialize(deserializer)?;

        let mut tiles: Vec<DungeonTile> = Vec::new();
        for (count, tile) in disk.runs {
            let room = MAX_GRID_CELLS.saturating_sub(tiles.len() as u64);
            let take = u64::from(count).min(room);
            tiles.extend(std::iter::repeat_n(tile, take as usize));
            if take < u64::from(count) {
                break;
            }
        }

        Ok(Self {
            width: disk.width,
            height: disk.height,
            metres_per_cell: disk.metres_per_cell,
            tiles,
        })
    }
}
