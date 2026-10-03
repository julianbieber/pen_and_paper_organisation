//! What the map decides about a cell, checked without a window.
//!
//! Most of it needs no terrain at all: the classifier is a function of a `CellFacts`
//! and a `HeightRamp`, so the awkward cases are stated directly rather than baked into
//! a fixture and hoped for.

use campaign::tiles::{
    self, CellFacts, ChunkScratch, ClaimedCell, DENSITY_STRENGTH, HeightRamp, MOUNTAIN_LINE,
    SHADE_STRENGTH, SNOW_LINE, TILE_COUNT, TileKind, Wet,
};
use glam::UVec2;
use watershed::{
    ChannelMeta, FieldInfo, FieldRole, LayerTexels, Terrain, TerrainLayer, WaterInfo,
};

const RAMP: HeightRamp = HeightRamp {
    low: 0.0,
    high: 60.0,
};

fn land(height: f32) -> CellFacts {
    CellFacts {
        height,
        wet: Wet::Dry,
        forest: 0.0,
        farmland: 0.0,
        dz_east: 0.0,
        dz_north: 0.0,
    }
}

fn water() -> CellFacts {
    CellFacts {
        wet: Wet::Sea,
        ..land(0.0)
    }
}

fn at_fraction(fraction: f32) -> CellFacts {
    land(RAMP.low + RAMP.width() * fraction)
}

// The one invariant that keeps the art and the classifier from drifting: the strip's
// columns are exactly the tiles the map can draw, with no gaps and no duplicates.
#[test]
fn every_tile_the_map_draws_has_exactly_one_column() {
    let mut seen = vec![false; TILE_COUNT as usize];
    for kind in TileKind::all() {
        let index = kind.index();
        assert!(
            index < TILE_COUNT,
            "{kind:?} indexes {index}, off a strip of {TILE_COUNT}"
        );
        assert!(!seen[index as usize], "{kind:?} collides at column {index}");
        seen[index as usize] = true;
    }
    assert!(
        seen.iter().all(|hit| *hit),
        "columns {:?} are on the strip but nothing can draw them",
        seen.iter()
            .enumerate()
            .filter(|(_, hit)| !**hit)
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    );
}

// The `water` field's three bands: dry at zero, a river or lake below one half, the sea
// from one half up; a value that is not a number says nothing.
#[test]
fn the_water_fields_bands_are_dry_fresh_and_sea() {
    assert_eq!(Wet::of_field(0.0), Some(Wet::Dry));
    assert_eq!(Wet::of_field(0.01), Some(Wet::Fresh));
    assert_eq!(Wet::of_field(0.45), Some(Wet::Fresh));
    assert_eq!(Wet::of_field(0.5), Some(Wet::Sea));
    assert_eq!(Wet::of_field(1.0), Some(Wet::Sea));
    assert_eq!(Wet::of_field(f32::NAN), None);
    assert_eq!(Wet::of_depth(0.0), Wet::Dry);
    assert_eq!(Wet::of_depth(0.3), Wet::Sea, "the solve cannot tell a river from the sea");
}

// Water beats every land tile, whatever the height or the densities say.
#[test]
fn water_is_drawn_as_water_whatever_else_holds() {
    let mut high = at_fraction(1.0);
    high.wet = Wet::Fresh;
    high.forest = 1.0;
    assert_eq!(tiles::classify(high, RAMP), TileKind::Water);
    assert_eq!(tiles::classify(water(), RAMP), TileKind::Water);
}

// The two height lines are fractions of the ramp, so mountains sit at the same place on
// any terrain whatever its height unit.
#[test]
fn mountain_and_snow_start_at_their_lines() {
    let below = MOUNTAIN_LINE - 0.01;
    assert_eq!(tiles::classify(at_fraction(below), RAMP), TileKind::Grass);
    assert_eq!(tiles::classify(at_fraction(MOUNTAIN_LINE), RAMP), TileKind::Mountain);
    assert_eq!(tiles::classify(at_fraction(SNOW_LINE - 0.01), RAMP), TileKind::Mountain);
    assert_eq!(tiles::classify(at_fraction(SNOW_LINE), RAMP), TileKind::Snow);
    assert_eq!(tiles::classify(at_fraction(1.0), RAMP), TileKind::Snow);
}

// Forest and farmland claim a cell only above half density, forest first, and never
// on a mountain: the height lines are what the GM reads as terrain.
#[test]
fn density_above_one_half_decides_forest_and_farmland() {
    let mut facts = land(10.0);
    facts.forest = 0.5;
    facts.farmland = 0.5;
    assert_eq!(tiles::classify(facts, RAMP), TileKind::Grass, "exactly one half is not above it");

    facts.farmland = 0.6;
    assert_eq!(tiles::classify(facts, RAMP), TileKind::Farmland);

    facts.forest = 0.6;
    assert_eq!(tiles::classify(facts, RAMP), TileKind::Forest, "forest wins over farmland");

    let mut high = at_fraction(MOUNTAIN_LINE);
    high.forest = 1.0;
    assert_eq!(tiles::classify(high, RAMP), TileKind::Mountain);
}

// The density shading: denser woods and fields are darker, the tint stays within its
// bound, and a tile that has no density is not tinted at all.
#[test]
fn a_denser_cell_is_drawn_darker_within_bounds() {
    let mut sparse = land(10.0);
    sparse.forest = 0.55;
    let mut dense = land(10.0);
    dense.forest = 1.0;

    let light = tiles::density_tint(TileKind::Forest, sparse);
    let dark = tiles::density_tint(TileKind::Forest, dense);
    assert!(dark < light, "{dark} is not darker than {light}");
    for tint in [light, dark] {
        assert!((tint - 1.0).abs() <= DENSITY_STRENGTH + f32::EPSILON, "{tint}");
    }
    assert_eq!(tiles::density_tint(TileKind::Grass, dense), 1.0);
}

// A flat terrain is constructible — `ChannelMeta::linear(v, v)` — and must not divide
// by zero into a tile that is not on the strip.
#[test]
fn a_ramp_with_no_width_reads_as_flat() {
    let flat = HeightRamp {
        low: 5.0,
        high: 5.0,
    };
    assert_eq!(flat.width(), 0.0);
    assert_eq!(tiles::classify(land(5.0), flat), TileKind::Grass);
    assert_eq!(tiles::shade(land(5.0), flat), 1.0);
}

// An inverted or absurd ramp must still land on the strip rather than panicking.
#[test]
fn heights_outside_the_ramp_still_land_on_the_strip() {
    let inverted = HeightRamp {
        low: 60.0,
        high: 0.0,
    };
    assert_eq!(inverted.width(), 0.0);
    assert_eq!(inverted.fraction(30.0), 0.0);

    for height in [f32::NAN, f32::INFINITY, -f32::INFINITY, -1e9, 1e9] {
        let fraction = RAMP.fraction(height);
        assert!((0.0..=1.0).contains(&fraction), "{height} gave {fraction}");
    }
}

// Shading is a tint with bounds: hand-drawn art is never blacked out or washed white,
// and water is left flat.
#[test]
fn a_shade_stays_within_reach_of_one_and_never_shades_water() {
    assert_eq!(tiles::shade(land(10.0), RAMP), 1.0, "flat ground is unlit");

    let mut steep = land(10.0);
    steep.dz_east = 1e6;
    steep.dz_north = -1e6;
    let mut away = land(10.0);
    away.dz_east = -1e6;
    away.dz_north = 1e6;

    for facts in [steep, away] {
        let shade = tiles::shade(facts, RAMP);
        assert!(shade > 0.0, "a tile is never blacked out");
        assert!((shade - 1.0).abs() <= SHADE_STRENGTH + f32::EPSILON, "{shade}");
    }
    assert!(
        tiles::shade(steep, RAMP) > tiles::shade(away, RAMP),
        "a slope facing the light is brighter than one facing away"
    );

    let mut wet = water();
    wet.dz_east = 1e6;
    assert_eq!(tiles::shade(wet, RAMP), 1.0, "water has no slope");
}

/// An 8x8 terrain whose height rises with the raster row, so raster row 0 — the bottom,
/// the south — is the lowest. The solve puts water on raster columns 0 and 1; each named
/// field is its own single-channel layer of raster bytes.
fn terrain_with(named: &[(&str, Vec<u8>)]) -> Terrain {
    let size = UVec2::new(8, 8);

    let heights: Vec<u8> = (0..64u32).map(|i| ((i / 8) * 32) as u8).collect();
    let mut layers = vec![TerrainLayer::new(
        Some(0),
        vec![ChannelMeta::linear(0.0, 255.0)],
        LayerTexels::from_bytes(size, 1, heights).expect("8x8x1 texels"),
    )];

    let mut water = Vec::with_capacity(64 * 4);
    for _ in 0..8u32 {
        for x in 0..8u32 {
            water.extend([if x < 2 { 255 } else { 0 }, 0, 0, 0]);
        }
    }
    layers.push(TerrainLayer::new(
        Some(0),
        vec![
            ChannelMeta::linear(0.0, 255.0),
            ChannelMeta::linear(-1.0, 1.0),
            ChannelMeta::linear(-1.0, 1.0),
            ChannelMeta::linear(0.0, 1000.0),
        ],
        LayerTexels::from_bytes(size, 4, water).expect("8x8x4 texels"),
    ));

    let mut fields = vec![FieldInfo {
        name: "height".to_owned(),
        role: FieldRole::Height,
        shift: 0,
        categorical: false,
        layer: 0,
        channel: 0,
    }];
    for (name, bytes) in named {
        fields.push(FieldInfo {
            name: (*name).to_owned(),
            role: FieldRole::Custom,
            shift: 0,
            categorical: false,
            layer: layers.len() as u8,
            channel: 0,
        });
        layers.push(TerrainLayer::new(
            Some(0),
            vec![ChannelMeta::linear(0.0, 1.0)],
            LayerTexels::from_bytes(size, 1, bytes.clone()).expect("8x8x1 texels"),
        ));
    }

    Terrain::new(size, fields, layers, Some(WaterInfo { lakes: 1, layer: 1 }))
}

/// The bytes of a field that is `value` on one raster cell and zero elsewhere.
fn one_cell(x: usize, raster_row: usize, value: u8) -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes[raster_row * 8 + x] = value;
    bytes
}

fn kind_at(chunk: &tiles::ChunkTiles, x: usize, document_row: usize) -> TileKind {
    let side = tiles::CHUNK_CELLS as usize;
    chunk.tiles[(side - 1 - document_row) * side + x]
        .expect("on the terrain")
        .kind
}

fn fill(terrain: &Terrain) -> tiles::ChunkTiles {
    let ramp = HeightRamp::of(terrain).expect("the fixture has a height field");
    tiles::chunk_tiles(terrain, ramp, 0, 0, &mut ChunkScratch::default())
}

// `watershed` stores a raster's bottom row first and a document's first row is its top,
// north: read the other way round, the whole map is upside down against the terrain
// editor — the forest the GM put in the north turns up in the south.
#[test]
fn the_rasters_last_row_is_the_documents_first() {
    assert_eq!(tiles::raster_row(0, 8), 7);
    assert_eq!(tiles::raster_row(7, 8), 0);

    let chunk = fill(&terrain_with(&[]));
    assert_eq!(kind_at(&chunk, 4, 0), TileKind::Snow, "the highest raster row is the north");
    assert_eq!(kind_at(&chunk, 4, 7), TileKind::Grass, "raster row 0 is the south");

    let forest = fill(&terrain_with(&[("forest", one_cell(4, 1, 255))]));
    assert_eq!(kind_at(&forest, 4, 6), TileKind::Forest);
    assert_eq!(kind_at(&forest, 4, 1), TileKind::Mountain);
}

// A terrain is not a whole number of chunks, so most chunks have cells that are not
// on it; those must stay undrawn rather than wrapping round.
#[test]
fn cells_off_the_terrain_are_left_undrawn() {
    let terrain = terrain_with(&[]);
    let ramp = HeightRamp::of(&terrain).expect("height field");
    let mut scratch = ChunkScratch::default();

    let chunk = tiles::chunk_tiles(&terrain, ramp, 0, 0, &mut scratch);
    let drawn = chunk.tiles.iter().filter(|tile| tile.is_some()).count();
    assert_eq!(drawn, 64, "only the terrain's 8x8 cells are drawn");

    let away = tiles::chunk_tiles(&terrain, ramp, -1, 2, &mut scratch);
    assert!(
        away.tiles.iter().all(|tile| tile.is_none()),
        "a chunk entirely off the terrain draws nothing, and negative coordinates do not wrap"
    );
}

// The `water` field is the authority where the terrain has one; the solve's depth is read
// only where it has none.
#[test]
fn the_water_field_wins_over_the_solve() {
    let solved = fill(&terrain_with(&[]));
    assert_eq!(kind_at(&solved, 0, 3), TileKind::Water, "the solve's water, with no field");
    assert_eq!(kind_at(&solved, 5, 7), TileKind::Grass);

    let authored = fill(&terrain_with(&[("water", one_cell(5, 0, 100))]));
    assert_eq!(kind_at(&authored, 5, 7), TileKind::Water, "the field's river");
    assert_eq!(kind_at(&authored, 0, 7), TileKind::Grass, "the solve is not read beside a field");
}

// Water runs through a city and is drawn over it; a road crosses a river or a lake as a
// bridge, but not the sea.
#[test]
fn a_claim_covers_land_and_a_road_bridges_fresh_water_only() {
    let mut field = one_cell(5, 0, 100);
    field[1] = 255;
    let mut chunk = fill(&terrain_with(&[("water", field)]));

    let side = tiles::CHUNK_CELLS;
    let slot = |x: u32, document_row: u32| (side - 1 - document_row) * side + x;
    chunk.claim(&[
        ClaimedCell { slot: slot(5, 7), kind: TileKind::City },
        ClaimedCell { slot: slot(4, 7), kind: TileKind::City },
    ]);
    assert_eq!(kind_at(&chunk, 5, 7), TileKind::Water, "a river runs through the city");
    assert_eq!(kind_at(&chunk, 4, 7), TileKind::City);

    chunk.claim(&[
        ClaimedCell { slot: slot(5, 7), kind: TileKind::Road },
        ClaimedCell { slot: slot(1, 7), kind: TileKind::Road },
    ]);
    assert_eq!(kind_at(&chunk, 5, 7), TileKind::Road, "a bridge over the river");
    assert_eq!(kind_at(&chunk, 1, 7), TileKind::Water, "no road over the sea");
}

// A terrain with no height field is the one worth refusing, and it must not panic.
#[test]
fn a_terrain_without_a_height_field_has_no_ramp() {
    assert_eq!(HeightRamp::of(&Terrain::new(UVec2::new(4, 4), vec![], vec![], None)), None);
    assert!(HeightRamp::of(&terrain_with(&[])).is_some());
}
