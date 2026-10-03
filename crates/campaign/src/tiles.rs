//! Which tile a cell is drawn as, and how brightly it is lit.
//!
//! Every decision the map makes about a cell is taken here rather than in the editor,
//! so all of it is checked without a window: the tile numbering, the coastline mask,
//! which cells a chunk covers, and which way up its rows go.
//!
//! Three vocabularies, one contract. A terrain cell is a [`TileKind`], a dungeon cell is
//! a [`DungeonTile`] and a combat map cell is a [`CombatTile`]; each numbers its own
//! strip, and `index` is the only place any of those numbers is written.

use serde::{Deserialize, Serialize};
use watershed::{FieldRole, Terrain};

use crate::grid::{TileGrid, TileVocabulary};

/// Steps the height range is divided into when measuring a slope for the hillshade.
///
/// A look parameter: a rise of one step across a cell is a steep slope whatever the
/// terrain's own height unit is.
pub const RELIEF_STEPS: u8 = 6;

/// The fraction of the height range above which dry land is mountain.
pub const MOUNTAIN_LINE: f32 = 0.55;

/// The fraction of the height range above which mountain carries snow.
pub const SNOW_LINE: f32 = 0.8;

/// The density above which a `forest` or `farmland` field claims a cell.
pub const DENSITY_THRESHOLD: f32 = 0.5;

/// How far a cell's density may move its forest or farmland tile's brightness from 1:
/// brightest just above [`DENSITY_THRESHOLD`], darkest at full density.
pub const DENSITY_STRENGTH: f32 = 0.15;

/// The terrain field whose density decides forest, read by name.
pub const FOREST_FIELD: &str = "forest";

/// The terrain field whose density decides farmland, read by name.
pub const FARMLAND_FIELD: &str = "farmland";

/// The terrain field saying where water lies, read by name: 0 is dry land, up to
/// [`SEA_LEVEL`] a river or lake, and from it upward the sea.
pub const WATER_FIELD: &str = "water";

/// Where the `water` field's sea band starts.
pub const SEA_LEVEL: f32 = 0.5;

/// Tiles the strip holds.
///
/// Every index [`TileKind::index`] can produce is below this, and every index below it
/// is one [`TileKind::all`] lists — so the strip has no tile that cannot be drawn and no
/// drawable tile missing from it.
pub const TILE_COUNT: u16 = 8;

/// Cells along one edge of a chunk.
pub const CHUNK_CELLS: u32 = 64;

/// How far a shade multiplier may move from 1 in either direction.
pub const SHADE_STRENGTH: f32 = 0.35;

/// How much a slope is steepened before it is lit.
///
/// A look parameter, not a measurement. Height differences between neighbouring cells
/// are a small fraction of a relief step on any terrain wide enough to be worth panning,
/// so lighting the true gradient would leave the map flat.
pub const RELIEF_EXAGGERATION: f32 = 6.0;

/// What a cell is drawn as.
///
/// The variants are the strip's columns in order, and [`TileKind::index`] is the only
/// place a tile number is written. [`TileKind::City`] and [`TileKind::Road`] come from
/// the features the GM drew, never from the terrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileKind {
    Grass,
    Farmland,
    Forest,
    Mountain,
    Snow,
    Water,
    City,
    Road,
}

impl TileKind {
    /// The tile's column in the strip, which is also its layer in the array texture.
    ///
    /// Always below [`TILE_COUNT`].
    pub fn index(self) -> u16 {
        match self {
            Self::Grass => 0,
            Self::Farmland => 1,
            Self::Forest => 2,
            Self::Mountain => 3,
            Self::Snow => 4,
            Self::Water => 5,
            Self::City => 6,
            Self::Road => 7,
        }
    }

    /// Every tile the map can draw, in strip order.
    ///
    /// The array length is the count, so adding a variant without extending this fails
    /// to compile.
    pub const fn all() -> [Self; TILE_COUNT as usize] {
        [
            Self::Grass,
            Self::Farmland,
            Self::Forest,
            Self::Mountain,
            Self::Snow,
            Self::Water,
            Self::City,
            Self::Road,
        ]
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

impl TileVocabulary for DungeonTile {
    const WALL: Self = Self::Wall;
    const FLOOR: Self = Self::Floor;

    fn index(self) -> u16 {
        DungeonTile::index(self)
    }
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

/// Tiles the combat strip holds.
///
/// Every index [`CombatTile::index`] can produce is below this and every index below it
/// is one it can produce, so the strip has no tile a combat map cannot paint.
pub const COMBAT_TILE_COUNT: u16 = 12;

/// What a combat map cell is drawn as.
///
/// The variants are `combat_tiles.png`'s columns in order, and [`CombatTile::index`] is
/// the only place one of its tile numbers is written. The strip is drawn by hand in
/// `bevy_sprite_editor`, so a redraw that keeps the order changes nothing here, and a
/// redraw that reorders it draws every combat map wrongly.
///
/// [`CombatTile::Grass`] is the default, which is what every cell of a new combat map is
/// filled with.
///
/// Deliberately not `#[non_exhaustive]`, for the reason [`TileKind`] is not.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CombatTile {
    #[default]
    Grass,
    Dirt,
    Road,
    Sand,
    Mud,
    ShallowWater,
    DeepWater,
    Tree,
    Bush,
    Boulder,
    Wall,
    Floor,
}

impl TileVocabulary for CombatTile {
    const WALL: Self = Self::Wall;
    const FLOOR: Self = Self::Floor;

    fn index(self) -> u16 {
        CombatTile::index(self)
    }
}

impl CombatTile {
    /// The tile's column in the combat strip, which is also its layer in the array
    /// texture.
    ///
    /// Always below [`COMBAT_TILE_COUNT`].
    pub fn index(self) -> u16 {
        match self {
            Self::Grass => 0,
            Self::Dirt => 1,
            Self::Road => 2,
            Self::Sand => 3,
            Self::Mud => 4,
            Self::ShallowWater => 5,
            Self::DeepWater => 6,
            Self::Tree => 7,
            Self::Bush => 8,
            Self::Boulder => 9,
            Self::Wall => 10,
            Self::Floor => 11,
        }
    }

    /// Every combat tile, in strip order.
    ///
    /// The array length is the count, so adding a variant without extending this fails
    /// to compile.
    pub const fn all() -> [Self; COMBAT_TILE_COUNT as usize] {
        [
            Self::Grass,
            Self::Dirt,
            Self::Road,
            Self::Sand,
            Self::Mud,
            Self::ShallowWater,
            Self::DeepWater,
            Self::Tree,
            Self::Bush,
            Self::Boulder,
            Self::Wall,
            Self::Floor,
        ]
    }

    /// The word for this tile in the tool strip, on the status line and on the control
    /// socket.
    pub fn label(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Dirt => "dirt",
            Self::Road => "road",
            Self::Sand => "sand",
            Self::Mud => "mud",
            Self::ShallowWater => "shallow water",
            Self::DeepWater => "deep water",
            Self::Tree => "tree",
            Self::Bush => "bush",
            Self::Boulder => "boulder",
            Self::Wall => "wall",
            Self::Floor => "floor",
        }
    }
}

/// The height range the mountain and snow lines are fractions of, taken from the
/// field's own range.
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

    /// How tall one [`RELIEF_STEPS`] step is, or zero on a ramp with no width.
    pub fn relief_step(self) -> f32 {
        self.width() / f32::from(RELIEF_STEPS)
    }

    /// Where a height sits on the ramp, from 0 at its low end to 1 at its high end.
    ///
    /// A ramp with no width, or a height that is not a number, answers 0, so a flat
    /// terrain reads as lowland rather than dividing by zero.
    pub fn fraction(self, height: f32) -> f32 {
        let width = self.width();
        if width <= 0.0 || !height.is_finite() {
            return 0.0;
        }
        ((height - self.low) / width).clamp(0.0, 1.0)
    }
}

/// Whether, and how, water lies on a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wet {
    #[default]
    Dry,
    /// A river or a lake, which a road may cross.
    Fresh,
    /// The sea, or water whose kind the terrain does not say.
    Sea,
}

impl Wet {
    /// What the `water` field's value says, or `None` where it is not a number.
    pub fn of_field(value: f32) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        Some(if value <= 0.0 {
            Self::Dry
        } else if value < SEA_LEVEL {
            Self::Fresh
        } else {
            Self::Sea
        })
    }

    /// What the water solve's depth says. It cannot tell a river from the sea, so any
    /// depth is [`Wet::Sea`] and no road crosses it.
    pub fn of_depth(depth: f32) -> Self {
        if depth > 0.0 { Self::Sea } else { Self::Dry }
    }

    /// Whether any water lies here.
    pub fn is_water(self) -> bool {
        self != Self::Dry
    }
}

/// Everything about one cell that decides what is drawn there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellFacts {
    pub height: f32,
    pub wet: Wet,
    /// The `forest` field's density here, or 0 on a terrain without one.
    pub forest: f32,
    /// The `farmland` field's density here, or 0 on a terrain without one.
    pub farmland: f32,
    /// How much the ground rises eastward across the cell, in height units per cell.
    pub dz_east: f32,
    /// How much the ground rises northward across the cell, in height units per cell.
    pub dz_north: f32,
}

impl CellFacts {
    /// Whether water lies on the cell.
    pub fn is_water(self) -> bool {
        self.wet.is_water()
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

/// What the terrain draws the cell as.
///
/// Water first, then snow and mountain by height, then forest and farmland where their
/// density passes [`DENSITY_THRESHOLD`], forest first, and grass otherwise. Never a tile
/// only a feature can claim.
pub fn classify(facts: CellFacts, ramp: HeightRamp) -> TileKind {
    if facts.is_water() {
        return TileKind::Water;
    }
    let up = ramp.fraction(facts.height);
    if up >= SNOW_LINE {
        return TileKind::Snow;
    }
    if up >= MOUNTAIN_LINE {
        return TileKind::Mountain;
    }
    if facts.forest > DENSITY_THRESHOLD {
        return TileKind::Forest;
    }
    if facts.farmland > DENSITY_THRESHOLD {
        return TileKind::Farmland;
    }
    TileKind::Grass
}

/// How brightly the cell is lit, as a multiplier on the tile's own colours.
///
/// Light comes from the north-west at a fixed angle; a slope facing it is brightened
/// and one facing away darkened, by at most [`SHADE_STRENGTH`]. The rise is measured
/// against the ramp's own relief step, so a terrain shades the same whether its heights
/// are metres or kilometres. Water is never shaded — it has no slope to catch the light,
/// and shading it would make the sea look like hills — and neither is anything on a ramp
/// with no width.
pub fn shade(facts: CellFacts, ramp: HeightRamp) -> f32 {
    let step = ramp.relief_step();
    if facts.is_water() || step <= 0.0 {
        return 1.0;
    }

    let gx = facts.dz_east / step * RELIEF_EXAGGERATION;
    let gy = facts.dz_north / step * RELIEF_EXAGGERATION;
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

/// The brightness `kind` takes from the cell's density, as a multiplier on its shade.
///
/// Forest and farmland only, and only above [`DENSITY_THRESHOLD`]: the denser the cell,
/// the darker, by at most [`DENSITY_STRENGTH`] either side of 1. Every other tile answers
/// 1.
pub fn density_tint(kind: TileKind, facts: CellFacts) -> f32 {
    let density = match kind {
        TileKind::Forest => facts.forest,
        TileKind::Farmland => facts.farmland,
        _ => return 1.0,
    };
    if !density.is_finite() {
        return 1.0;
    }
    let over = ((density - DENSITY_THRESHOLD) / (1.0 - DENSITY_THRESHOLD)).clamp(0.0, 1.0);
    1.0 + DENSITY_STRENGTH * (1.0 - 2.0 * over)
}

fn lit(shade: f32) -> f32 {
    shade.clamp(1.0 - SHADE_STRENGTH, 1.0 + SHADE_STRENGTH)
}

/// What a feature's claim needs to know about a cell after it has been drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellCache {
    pub wet: Wet,
    /// The hillshade alone, without a density tint.
    pub shade: f32,
}

/// One chunk's worth of drawn cells.
///
/// Both vectors are `CHUNK_CELLS * CHUNK_CELLS` long and indexed `row * CHUNK_CELLS +
/// column` with **row zero at the bottom** — the tilemap's order, not the document's.
/// `None` is a cell outside the terrain, which is left undrawn so a terrain whose
/// extent is not a whole number of chunks has a ragged edge rather than a wrapped one.
#[derive(Debug, Clone)]
pub struct ChunkTiles {
    pub tiles: Vec<Option<MapTile>>,
    pub cells: Vec<Option<CellCache>>,
}

impl ChunkTiles {
    /// Draw the cells `claims` holds for this chunk as the tile each one is claimed for.
    ///
    /// Water runs through a city and is drawn over it. A road is drawn over a river or a
    /// lake, the way a bridge is, but never over the sea.
    pub fn claim(&mut self, claims: &[ClaimedCell]) {
        for claimed in claims {
            let slot = claimed.slot as usize;
            let Some(Some(cell)) = self.cells.get(slot) else {
                continue;
            };
            let covers = match cell.wet {
                Wet::Dry => true,
                Wet::Fresh => claimed.kind == TileKind::Road,
                Wet::Sea => false,
            };
            if covers {
                self.tiles[slot] = Some(MapTile {
                    kind: claimed.kind,
                    shade: cell.shade,
                });
            }
        }
    }
}

/// A cell a feature claims, by its slot in a chunk's tilemap-ordered vectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimedCell {
    pub slot: u32,
    pub kind: TileKind,
}

/// Reusable room for one chunk's cells and the ring of neighbours around them.
///
/// Owned by whoever fills chunks and passed back in, so filling a chunk allocates
/// nothing.
#[derive(Debug, Default)]
pub struct ChunkScratch {
    height: Vec<f32>,
    inside: Vec<bool>,
}

const APRON: u32 = CHUNK_CELLS + 2;

impl ChunkScratch {
    fn reset(&mut self) {
        let cells = (APRON * APRON) as usize;
        self.height.clear();
        self.height.resize(cells, 0.0);
        self.inside.clear();
        self.inside.resize(cells, false);
    }

    fn at(row: i64, column: i64) -> usize {
        ((row + 1) * i64::from(APRON) + column + 1) as usize
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

/// The raster row `watershed` stores a document row in.
///
/// A document's rows run top to bottom, north first, and a raster's row zero is its
/// bottom — so document row `y` is raster row `height - 1 - y`. Every terrain read goes
/// through this, and nothing else in the workspace flips a terrain row.
pub fn raster_row(y: u32, height: u32) -> u32 {
    height - 1 - y
}

/// The tiles of the chunk at `chunk_x, chunk_y`, in tilemap order.
///
/// Reads the chunk's heights and the one-cell ring around them out of the terrain once,
/// so no cell is asked for five times over. A document's rows run top to bottom and a
/// chunk's run bottom to top, so this is where a document row is flipped into a chunk;
/// the read from the terrain goes through [`raster_row`]. A chunk entirely off the
/// terrain comes back all `None` rather than as an error; chunk coordinates are signed
/// and a camera panned past the origin reaches them.
///
/// Water is the `water` field when the terrain has one and the water solve's depth when
/// it does not. The `forest` and `farmland` fields are read by name when present.
pub fn chunk_tiles(
    terrain: &Terrain,
    ramp: HeightRamp,
    chunk_x: i32,
    chunk_y: i32,
    scratch: &mut ChunkScratch,
) -> ChunkTiles {
    scratch.reset();

    let side = i64::from(CHUNK_CELLS);
    let origin_x = i64::from(chunk_x) * side;
    let origin_y = i64::from(chunk_y) * side;
    let width = i64::from(terrain.width());
    let height_extent = i64::from(terrain.height());
    let field = terrain.field_with_role(FieldRole::Height);
    let forest = terrain.field(FOREST_FIELD);
    let farmland = terrain.field(FARMLAND_FIELD);
    let water_field = terrain.field(WATER_FIELD);
    let solve = terrain.water();
    let raster = |x: i64, y: i64| (x as u32, raster_row(y as u32, terrain.height()));

    for row in -1..=side {
        for column in -1..=side {
            let (x, y) = (origin_x + column, origin_y + row);
            if x < 0 || y < 0 || x >= width || y >= height_extent {
                continue;
            }
            let (x, y) = raster(x, y);
            let index = ChunkScratch::at(row, column);
            scratch.inside[index] = true;
            scratch.height[index] = field.and_then(|f| f.value_at(x, y)).unwrap_or(0.0);
        }
    }

    let cells = (CHUNK_CELLS * CHUNK_CELLS) as usize;
    let mut tiles = vec![None; cells];
    let mut caches = vec![None; cells];

    for row in 0..side {
        for column in 0..side {
            let index = ChunkScratch::at(row, column);
            if !scratch.inside[index] {
                continue;
            }
            let (x, y) = raster(origin_x + column, origin_y + row);

            let own = scratch.height[index];
            let east = scratch.height_at(row, column + 1, own);
            let west = scratch.height_at(row, column - 1, own);
            let north = scratch.height_at(row - 1, column, own);
            let south = scratch.height_at(row + 1, column, own);

            let density = |view: Option<watershed::FieldView>| {
                view.and_then(|f| f.value_at(x, y)).unwrap_or(0.0)
            };
            let wet = match water_field {
                Some(view) => view.value_at(x, y).and_then(Wet::of_field).unwrap_or_default(),
                None => solve
                    .and_then(|solve| solve.depth_at(x, y))
                    .map(Wet::of_depth)
                    .unwrap_or_default(),
            };
            let facts = CellFacts {
                height: own,
                wet,
                forest: density(forest),
                farmland: density(farmland),
                dz_east: (east - west) / 2.0,
                dz_north: (north - south) / 2.0,
            };

            let hill = shade(facts, ramp);
            let kind = classify(facts, ramp);
            let slot = ((side - 1 - row) * side + column) as usize;
            caches[slot] = Some(CellCache { wet, shade: hill });
            tiles[slot] = Some(MapTile {
                kind,
                shade: lit(hill * density_tint(kind, facts)),
            });
        }
    }

    ChunkTiles {
        tiles,
        cells: caches,
    }
}

/// The grid tiles of the chunk at `chunk_x, chunk_y`, in tilemap order.
///
/// The grid counterpart of [`chunk_tiles`], and it makes the same two promises: the row
/// flip happens here, and a cell outside the grid comes back `None` rather than as an
/// error — so a grid whose extent is not a whole number of chunks has a ragged edge, and
/// a chunk entirely off the grid draws nothing. `None` and the vocabulary's [`Default`]
/// tile stay different answers: unexcavated rock is a tile the GM can paint over, and off
/// the grid is not.
///
/// Chunk coordinates are signed, so every cell is bounded through
/// [`TileGrid::get`](crate::grid::TileGrid::get), which checks each axis on its own.
pub fn grid_chunk_tiles<T: TileVocabulary>(
    grid: &TileGrid<T>,
    chunk_x: i32,
    chunk_y: i32,
) -> Vec<Option<T>> {
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
