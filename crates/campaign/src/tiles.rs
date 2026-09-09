//! Which tile a cell is drawn as, and how brightly it is lit.
//!
//! Every decision the map makes about a cell is taken here rather than in the editor,
//! so all of it is checked without a window: the tile numbering, the coastline mask,
//! which cells a chunk covers, and which way up its rows go.
//!
//! Two vocabularies, one contract. A terrain cell is a [`TileKind`] and a dungeon cell
//! is a [`DungeonTile`]; each numbers its own strip, and `index` is the only place
//! either number is written.

use serde::{Deserialize, Serialize};
use watershed::{FieldRole, Terrain};

use crate::grid::TileGrid;

/// Bands the height range is quantised into.
pub const LAND_BANDS: u8 = 6;

/// Tiles the strip holds.
///
/// Every index [`classify`] can produce is below this, and every index below it is one
/// [`classify`] can produce — so the strip has no tile that cannot be drawn and no
/// drawable tile missing from it.
pub const TILE_COUNT: u16 = 24;

/// Cells along one edge of a chunk.
pub const CHUNK_CELLS: u32 = 64;

/// How far a shade multiplier may move from 1 in either direction.
pub const SHADE_STRENGTH: f32 = 0.35;

/// How much a slope is steepened before it is lit.
///
/// A look parameter, not a measurement. Height differences between neighbouring cells
/// are a small fraction of a band on any terrain wide enough to be worth panning, so
/// lighting the true gradient would leave the map flat.
pub const RELIEF_EXAGGERATION: f32 = 6.0;

/// What a cell is drawn as.
///
/// The variants are the strip's columns in order, and [`TileKind::index`] is the only
/// place a tile number is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    /// Dry land, banded from lowest to highest. Always below [`LAND_BANDS`].
    Land(u8),
    ShallowWater,
    DeepWater,
    River,
    /// Land touching water, keyed by which of its neighbours are wet. Always
    /// `1..=15` — a cell with no wet neighbour is not a coast.
    Coast(u8),
}

impl TileKind {
    /// The tile's column in the strip, which is also its layer in the array texture.
    ///
    /// Panics in debug builds on a band at or above [`LAND_BANDS`] or a coast mask
    /// outside `1..=15`; both are unconstructible from [`classify`].
    pub fn index(self) -> u16 {
        match self {
            Self::Land(band) => {
                debug_assert!(band < LAND_BANDS, "land band {band} is off the strip");
                u16::from(band)
            }
            Self::ShallowWater => u16::from(LAND_BANDS),
            Self::DeepWater => u16::from(LAND_BANDS) + 1,
            Self::River => u16::from(LAND_BANDS) + 2,
            Self::Coast(mask) => {
                debug_assert!(
                    (1..=15).contains(&mask),
                    "coast mask {mask} cannot occur: a coast has at least one wet neighbour"
                );
                u16::from(LAND_BANDS) + 2 + u16::from(mask)
            }
        }
    }

    /// Every tile the map can draw, in strip order.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..LAND_BANDS)
            .map(Self::Land)
            .chain([Self::ShallowWater, Self::DeepWater, Self::River])
            .chain((1..=15).map(Self::Coast))
    }
}

/// Tiles the dungeon strip holds.
///
/// Every index [`DungeonTile::index`] can produce is below this and every index below
/// it is one it can produce, which is what lets the strip be built by walking
/// [`DungeonTile::all`].
pub const DUNGEON_TILE_COUNT: u16 = 9;

/// What a dungeon cell is drawn as.
///
/// The variants are the dungeon strip's columns in order, and [`DungeonTile::index`] is
/// the only place one of its tile numbers is written — the same contract [`TileKind`]
/// carries for the terrain strip.
///
/// [`DungeonTile::Empty`] is unexcavated rock and is a tile like any other. A grid holds
/// one for every cell it covers, so "empty" and "outside the grid" stay different
/// answers: the second is what [`TileGrid::get`](crate::grid::TileGrid::get) reports as
/// `None`.
///
/// Deliberately not `#[non_exhaustive]`, for the reason [`TileKind`] is not: a consumer
/// matching on a tile to decide how to draw it should fail to compile when one is added.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DungeonTile {
    #[default]
    Empty,
    Floor,
    Wall,
    Door,
    SecretDoor,
    StairsUp,
    StairsDown,
    Water,
    Rubble,
}

impl DungeonTile {
    /// The tile's column in the dungeon strip, which is also its layer in the array
    /// texture.
    ///
    /// Always below [`DUNGEON_TILE_COUNT`].
    pub fn index(self) -> u16 {
        match self {
            Self::Empty => 0,
            Self::Floor => 1,
            Self::Wall => 2,
            Self::Door => 3,
            Self::SecretDoor => 4,
            Self::StairsUp => 5,
            Self::StairsDown => 6,
            Self::Water => 7,
            Self::Rubble => 8,
        }
    }

    /// Every dungeon tile, in strip order.
    ///
    /// Written out rather than derived, and the array length is the count — adding a
    /// variant without extending this fails to compile.
    pub const fn all() -> [Self; DUNGEON_TILE_COUNT as usize] {
        [
            Self::Empty,
            Self::Floor,
            Self::Wall,
            Self::Door,
            Self::SecretDoor,
            Self::StairsUp,
            Self::StairsDown,
            Self::Water,
            Self::Rubble,
        ]
    }

    /// The word for this tile in the tool strip and on the status line.
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Floor => "floor",
            Self::Wall => "wall",
            Self::Door => "door",
            Self::SecretDoor => "secret door",
            Self::StairsUp => "stairs up",
            Self::StairsDown => "stairs down",
            Self::Water => "water",
            Self::Rubble => "rubble",
        }
    }
}

/// The height range the bands are spread over, taken from the field's own range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightRamp {
    pub low: f32,
    pub high: f32,
}

impl HeightRamp {
    /// The ramp a terrain's height field asks for, or `None` when it has no height
    /// field or no extent at all.
    pub fn of(terrain: &Terrain) -> Option<Self> {
        if terrain.width() == 0 || terrain.height() == 0 {
            return None;
        }
        let height = terrain.field_with_role(FieldRole::Height)?;
        Some(Self {
            low: height.range_low(),
            high: height.range_high(),
        })
    }

    /// How much ground the ramp spans. Zero when the terrain is flat, which is
    /// constructible and must not divide anything.
    pub fn width(self) -> f32 {
        (self.high - self.low).max(0.0)
    }

    /// How tall one band is, or zero on a ramp with no width.
    pub fn band_height(self) -> f32 {
        self.width() / f32::from(LAND_BANDS)
    }

    /// Which band a height falls in, clamped into the ramp.
    ///
    /// A ramp with no width puts everything in the lowest band, so a flat terrain
    /// reads as flat rather than dividing by zero into a tile that is not on the strip.
    pub fn band(self, height: f32) -> u8 {
        let width = self.width();
        if width <= 0.0 || !height.is_finite() {
            return 0;
        }
        let fraction = ((height - self.low) / width).clamp(0.0, 1.0);
        ((fraction * f32::from(LAND_BANDS)) as u8).min(LAND_BANDS - 1)
    }
}

/// Everything about one cell that decides what is drawn there.
///
/// `depth` is `Some` exactly when the water solve says water stands on the cell, so
/// `depth.is_some()` is the water test — `watershed` defines `is_water` as a positive
/// depth, and reading it back from the depth avoids asking twice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellFacts {
    pub height: f32,
    pub depth: Option<f32>,
    pub accumulation: f32,
    /// Which of the four neighbours are water: bit 0 north, 1 east, 2 south, 3 west,
    /// in terrain rows, before any flip. A neighbour off the terrain counts as land.
    pub water_mask: u8,
    /// How much the ground rises eastward across the cell, in height units per cell.
    pub dz_east: f32,
    /// How much the ground rises northward across the cell, in height units per cell.
    pub dz_north: f32,
}

impl CellFacts {
    /// Whether water stands on the cell.
    pub fn is_water(self) -> bool {
        self.depth.is_some()
    }
}

/// A drawn cell: which tile, and how brightly lit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapTile {
    pub kind: TileKind,
    /// A multiplier on the tile's own colours, always above zero and within
    /// [`SHADE_STRENGTH`] of 1.
    pub shade: f32,
}

/// What the cell is drawn as when nothing is a river.
///
/// Never returns [`TileKind::River`], so a threshold move can choose between this and
/// a river without reading the terrain again.
pub fn dry_kind(facts: CellFacts, ramp: HeightRamp) -> TileKind {
    if let Some(depth) = facts.depth {
        return if depth > ramp.band_height() && ramp.width() > 0.0 {
            TileKind::DeepWater
        } else {
            TileKind::ShallowWater
        };
    }
    if facts.water_mask != 0 {
        return TileKind::Coast(facts.water_mask);
    }
    TileKind::Land(ramp.band(facts.height))
}

/// What the cell is drawn as, given what currently counts as a river.
///
/// Water wins over everything, then a channel, then a coastline, then the height band.
/// A river reaching the sea is a mouth: drawing it as a channel keeps the drainage
/// readable through the coastline. The comparison is strictly greater, so a threshold
/// of zero does not turn every cell — including every read off the edge of the
/// terrain, which `watershed` reports as zero accumulation — into a river.
pub fn classify(facts: CellFacts, ramp: HeightRamp, threshold: f32) -> TileKind {
    if !facts.is_water() && facts.accumulation > threshold {
        return TileKind::River;
    }
    dry_kind(facts, ramp)
}

/// How brightly the cell is lit, as a multiplier on the tile's own colours.
///
/// Light comes from the north-west at a fixed angle; a slope facing it is brightened
/// and one facing away darkened, by at most [`SHADE_STRENGTH`]. The rise is measured
/// against the ramp's own bands, so a terrain shades the same whether its heights are
/// metres or kilometres. Standing water is never shaded — it has no slope to catch the
/// light, and shading it would make the sea look like hills — and neither is anything
/// on a ramp with no width.
pub fn shade(facts: CellFacts, ramp: HeightRamp) -> f32 {
    let band = ramp.band_height();
    if facts.is_water() || band <= 0.0 {
        return 1.0;
    }

    let gx = facts.dz_east / band * RELIEF_EXAGGERATION;
    let gy = facts.dz_north / band * RELIEF_EXAGGERATION;
    if !gx.is_finite() || !gy.is_finite() {
        return 1.0;
    }

    const DIAGONAL: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let length = (gx * gx + gy * gy + 1.0).sqrt();
    let lit = (gx * DIAGONAL - gy * DIAGONAL + 1.0) * DIAGONAL / length;

    let flat = DIAGONAL;
    let offset = (lit - flat) / (1.0 - flat);
    (1.0 + SHADE_STRENGTH * offset).clamp(1.0 - SHADE_STRENGTH, 1.0 + SHADE_STRENGTH)
}

/// What is kept beside a chunk so a threshold move need not read the terrain again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellCache {
    /// The tile this cell takes when it is not a river.
    pub dry: TileKind,
    pub shade: f32,
    /// The accumulation a threshold is compared against, or `None` on a cell that can
    /// never be a river — standing water, whatever the threshold does.
    pub river_accumulation: Option<f32>,
}

/// One chunk's worth of drawn cells, and enough beside them to re-choose their tiles.
///
/// Both vectors are `CHUNK_CELLS * CHUNK_CELLS` long and indexed `row * CHUNK_CELLS +
/// column` with **row zero at the bottom** — the tilemap's order, not the terrain's.
/// `None` is a cell outside the terrain, which is left undrawn so a terrain whose
/// extent is not a whole number of chunks has a ragged edge rather than a wrapped one.
#[derive(Debug, Clone)]
pub struct ChunkTiles {
    pub tiles: Vec<Option<MapTile>>,
    pub cells: Vec<Option<CellCache>>,
    /// The lowest and highest accumulation on this chunk's land, or `None` when it has
    /// none — the test for whether a threshold move can change anything here.
    pub land_accumulation: Option<(f32, f32)>,
}

impl ChunkTiles {
    /// Whether moving the threshold from `from` to `to` can change any tile here.
    ///
    /// False for a chunk whose land accumulation lies entirely above or entirely below
    /// both thresholds, which is most of a map most of the time.
    pub fn affected_by(&self, from: f32, to: f32) -> bool {
        let Some((low, high)) = self.land_accumulation else {
            return false;
        };
        let (near, far) = if from <= to { (from, to) } else { (to, from) };
        low <= far && high > near
    }

    /// Re-choose every tile against a new threshold, without touching the terrain.
    ///
    /// Returns whether anything actually changed, so an unchanged chunk is not marked
    /// dirty and re-uploaded.
    pub fn apply_threshold(&mut self, threshold: f32) -> bool {
        let mut changed = false;
        for (tile, cell) in self.tiles.iter_mut().zip(self.cells.iter()) {
            let Some(cell) = cell else { continue };
            let kind = match cell.river_accumulation {
                Some(accumulation) if accumulation > threshold => TileKind::River,
                _ => cell.dry,
            };
            let next = MapTile {
                kind,
                shade: cell.shade,
            };
            if *tile != Some(next) {
                *tile = Some(next);
                changed = true;
            }
        }
        changed
    }
}

/// Reusable room for one chunk's cells and the ring of neighbours around them.
///
/// Owned by whoever fills chunks and passed back in, so filling a chunk allocates
/// nothing.
#[derive(Debug, Default)]
pub struct ChunkScratch {
    height: Vec<f32>,
    depth: Vec<f32>,
    accumulation: Vec<f32>,
    inside: Vec<bool>,
}

const APRON: u32 = CHUNK_CELLS + 2;

impl ChunkScratch {
    fn reset(&mut self) {
        let cells = (APRON * APRON) as usize;
        self.height.clear();
        self.height.resize(cells, 0.0);
        self.depth.clear();
        self.depth.resize(cells, 0.0);
        self.accumulation.clear();
        self.accumulation.resize(cells, 0.0);
        self.inside.clear();
        self.inside.resize(cells, false);
    }

    fn at(row: i64, column: i64) -> usize {
        ((row + 1) * i64::from(APRON) + column + 1) as usize
    }

    fn is_water(&self, row: i64, column: i64) -> bool {
        let index = Self::at(row, column);
        self.inside[index] && self.depth[index] > 0.0
    }

    fn height_at(&self, row: i64, column: i64, fallback: f32) -> f32 {
        let index = Self::at(row, column);
        if self.inside[index] {
            self.height[index]
        } else {
            fallback
        }
    }
}

/// The tiles of the chunk at `chunk_x, chunk_y`, in tilemap order.
///
/// Reads the chunk's cells and the one-cell ring around them out of the terrain once,
/// so no cell is asked for five times over. The terrain's rows run top to bottom and a
/// chunk's run bottom to top, so this is where — and the only place where — a row is
/// flipped. A chunk entirely off the terrain comes back all `None` rather than as an
/// error; chunk coordinates are signed and a camera panned past the origin reaches
/// them.
pub fn chunk_tiles(
    terrain: &Terrain,
    ramp: HeightRamp,
    chunk_x: i32,
    chunk_y: i32,
    threshold: f32,
    scratch: &mut ChunkScratch,
) -> ChunkTiles {
    scratch.reset();

    let side = i64::from(CHUNK_CELLS);
    let origin_x = i64::from(chunk_x) * side;
    let origin_y = i64::from(chunk_y) * side;
    let width = i64::from(terrain.width());
    let height_extent = i64::from(terrain.height());
    let field = terrain.field_with_role(FieldRole::Height);
    let water = terrain.water();

    for row in -1..=side {
        for column in -1..=side {
            let (x, y) = (origin_x + column, origin_y + row);
            if x < 0 || y < 0 || x >= width || y >= height_extent {
                continue;
            }
            let (x, y) = (x as u32, y as u32);
            let index = ChunkScratch::at(row, column);
            scratch.inside[index] = true;
            scratch.height[index] = field.and_then(|f| f.value_at(x, y)).unwrap_or(0.0);
            if let Some(water) = water {
                scratch.depth[index] = water.depth_at(x, y).unwrap_or(0.0);
                scratch.accumulation[index] = water.accumulation(x, y);
            }
        }
    }

    let cells = (CHUNK_CELLS * CHUNK_CELLS) as usize;
    let mut tiles = vec![None; cells];
    let mut caches = vec![None; cells];
    let mut land_accumulation: Option<(f32, f32)> = None;

    for row in 0..side {
        for column in 0..side {
            let index = ChunkScratch::at(row, column);
            if !scratch.inside[index] {
                continue;
            }

            let own = scratch.height[index];
            let depth = scratch.depth[index];
            let accumulation = scratch.accumulation[index];

            let mut water_mask = 0u8;
            water_mask |= u8::from(scratch.is_water(row - 1, column));
            water_mask |= u8::from(scratch.is_water(row, column + 1)) << 1;
            water_mask |= u8::from(scratch.is_water(row + 1, column)) << 2;
            water_mask |= u8::from(scratch.is_water(row, column - 1)) << 3;

            let east = scratch.height_at(row, column + 1, own);
            let west = scratch.height_at(row, column - 1, own);
            let north = scratch.height_at(row - 1, column, own);
            let south = scratch.height_at(row + 1, column, own);

            let facts = CellFacts {
                height: own,
                depth: (depth > 0.0).then_some(depth),
                accumulation,
                water_mask,
                dz_east: (east - west) / 2.0,
                dz_north: (north - south) / 2.0,
            };

            let cache = CellCache {
                dry: dry_kind(facts, ramp),
                shade: shade(facts, ramp),
                river_accumulation: (!facts.is_water()).then_some(accumulation),
            };
            if cache.river_accumulation.is_some() {
                land_accumulation = Some(match land_accumulation {
                    Some((low, high)) => (low.min(accumulation), high.max(accumulation)),
                    None => (accumulation, accumulation),
                });
            }

            let slot = ((side - 1 - row) * side + column) as usize;
            tiles[slot] = Some(MapTile {
                kind: classify(facts, ramp, threshold),
                shade: cache.shade,
            });
            caches[slot] = Some(cache);
        }
    }

    ChunkTiles {
        tiles,
        cells: caches,
        land_accumulation,
    }
}

/// The highest accumulation the terrain reaches, estimated from a strided sample.
///
/// Accumulation counts everything draining through a cell, so its size follows the
/// terrain's and a compiled-in threshold range would be all-river on one terrain and
/// all-dry on the next. Sampled rather than scanned: a terrain several thousand cells
/// on a side has tens of millions of cells, and this runs while a window is open.
/// `None` when the terrain has no water solve.
pub fn accumulation_ceiling(terrain: &Terrain) -> Option<f32> {
    const TARGET_SAMPLES: u32 = 256;

    let water = terrain.water()?;
    let (width, height) = (terrain.width(), terrain.height());
    if width == 0 || height == 0 {
        return None;
    }

    let stride_x = (width / TARGET_SAMPLES).max(1);
    let stride_y = (height / TARGET_SAMPLES).max(1);
    let mut ceiling = 0.0f32;
    let mut y = 0;
    while y < height {
        let mut x = 0;
        while x < width {
            ceiling = ceiling.max(water.accumulation(x, y));
            x += stride_x;
        }
        y += stride_y;
    }
    Some(ceiling)
}

/// The dungeon tiles of the chunk at `chunk_x, chunk_y`, in tilemap order.
///
/// The grid counterpart of [`chunk_tiles`], and it makes the same two promises: the row
/// flip happens here, and a cell outside the grid comes back `None` rather than as an
/// error — so a grid whose extent is not a whole number of chunks has a ragged edge, and
/// a chunk entirely off the grid draws nothing. `None` and
/// [`DungeonTile::Empty`] stay different answers: unexcavated rock is a tile the GM can
/// paint over, and off the grid is not.
///
/// Chunk coordinates are signed, so every cell is bounded through
/// [`TileGrid::get`](crate::grid::TileGrid::get), which checks each axis on its own.
pub fn grid_chunk_tiles(grid: &TileGrid, chunk_x: i32, chunk_y: i32) -> Vec<Option<DungeonTile>> {
    let side = i64::from(CHUNK_CELLS);
    let origin_x = i64::from(chunk_x) * side;
    let origin_y = i64::from(chunk_y) * side;

    let mut tiles = vec![None; (CHUNK_CELLS * CHUNK_CELLS) as usize];
    for row in 0..side {
        for column in 0..side {
            let tile = grid.get(origin_x + column, origin_y + row);
            let slot = ((side - 1 - row) * side + column) as usize;
            tiles[slot] = tile;
        }
    }
    tiles
}

/// How strongly each level of the grid is drawn at `cells_per_pixel`.
///
/// Two levels, both faded, because one is not enough. A single spacing that switched from
/// every cell to every fifth would jump on the frame it crossed its threshold, and every
/// other threshold in this workspace fades for exactly that reason. Fading one level out
/// and the next in independently also produces the look a battle map is conventionally
/// ruled in: the major lines are drawn under the minor ones and so read brighter, without
/// anything deciding that they should.
///
/// A strength of zero means that level is not drawn at all, so a grid zoomed far enough
/// out disappears rather than becoming a wash of colour.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GridLines {
    /// How strongly a line on every cell boundary is drawn, from zero to one.
    pub fine: f32,
    /// How strongly a line every [`GRID_COARSE_CELLS`] cells is drawn, from zero to one.
    pub coarse: f32,
}

impl GridLines {
    /// Whether neither level draws, which is the answer at every scale coarser than a
    /// grid is worth ruling at.
    pub fn draws_nothing(self) -> bool {
        self.fine <= 0.0 && self.coarse <= 0.0
    }
}

/// How strongly to draw each level of the grid, given what a logical pixel is worth in
/// cells.
///
/// Both levels come back at zero for a scale that is not a finite positive number, so a
/// pointer that has not measured anything yet draws no grid rather than an infinite one.
pub fn grid_lines(cells_per_pixel: f32) -> GridLines {
    if !cells_per_pixel.is_finite() || cells_per_pixel <= 0.0 {
        return GridLines::default();
    }
    let pixels_per_cell = 1.0 / cells_per_pixel;

    let strength = |spacing: u32| {
        let apart = pixels_per_cell * spacing as f32;
        ((apart / GRID_MIN_PIXELS - 1.0) / (GRID_FADE_SPAN - 1.0)).clamp(0.0, 1.0)
    };

    GridLines {
        fine: strength(1),
        coarse: strength(GRID_COARSE_CELLS),
    }
}

/// How far apart two grid lines must be on screen before they are drawn at all, in
/// logical pixels.
pub const GRID_MIN_PIXELS: f32 = 5.0;

/// How many cells the coarse level steps by.
///
/// Five, which is the major line a battle map is conventionally ruled in, so the coarse
/// grid reads as a deliberate scale rather than as a degraded one.
pub const GRID_COARSE_CELLS: u32 = 5;

/// How far past [`GRID_MIN_PIXELS`] a level has to open out before it is drawn at full
/// strength.
pub const GRID_FADE_SPAN: f32 = 2.0;
