//! What the map decides about a cell, checked without a window.
//!
//! Most of it needs no terrain at all: the classifier is a function of a `CellFacts`
//! and a `HeightRamp`, so the awkward cases are stated directly rather than baked into
//! a fixture and hoped for.

use campaign::tiles::{
    self, CellFacts, ChunkScratch, ClaimedCell, DENSITY_STRENGTH, HeightRamp, MOUNTAIN_LINE,
    SHADE_STRENGTH, SNOW_LINE, TILE_COUNT, TileKind,
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
        depth: None,
        accumulation: 0.0,
        forest: 0.0,
        farmland: 0.0,
        dz_east: 0.0,
        dz_north: 0.0,
    }
}

fn water(depth: f32) -> CellFacts {
    CellFacts {
        depth: Some(depth),
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

// Water beats everything, whatever the threshold says about the water's accumulation.
#[test]
fn standing_water_is_never_a_river() {
    let mut facts = water(1.0);
    facts.accumulation = f32::MAX;
    assert_eq!(tiles::classify(facts, RAMP, 0.0), TileKind::Water);
}

// `watershed` reports zero accumulation off the edge of the terrain and its own
// channel test is `>=`, so a threshold of zero must not turn the whole map into river.
#[test]
fn a_threshold_of_zero_does_not_make_every_cell_a_river() {
    assert_eq!(
        tiles::classify(land(10.0), RAMP, 0.0),
        TileKind::Grass,
        "a dry cell with no accumulation is not a channel at threshold zero"
    );

    let mut flowing = land(10.0);
    flowing.accumulation = 0.5;
    assert_eq!(tiles::classify(flowing, RAMP, 0.0), TileKind::River);
}

// The two height lines are fractions of the ramp, so mountains sit at the same place on
// any terrain whatever its height unit.
#[test]
fn mountain_and_snow_start_at_their_lines() {
    let below = MOUNTAIN_LINE - 0.01;
    assert_eq!(tiles::dry_kind(at_fraction(below), RAMP), TileKind::Grass);
    assert_eq!(
        tiles::dry_kind(at_fraction(MOUNTAIN_LINE), RAMP),
        TileKind::Mountain
    );
    assert_eq!(
        tiles::dry_kind(at_fraction(SNOW_LINE - 0.01), RAMP),
        TileKind::Mountain
    );
    assert_eq!(
        tiles::dry_kind(at_fraction(SNOW_LINE), RAMP),
        TileKind::Snow
    );
    assert_eq!(tiles::dry_kind(at_fraction(1.0), RAMP), TileKind::Snow);
}

// Forest and farmland claim a cell only above half density, forest first, and never
// on a mountain: the height lines are what the GM reads as terrain.
#[test]
fn density_above_one_half_decides_forest_and_farmland() {
    let mut facts = land(10.0);
    facts.forest = 0.5;
    facts.farmland = 0.5;
    assert_eq!(
        tiles::dry_kind(facts, RAMP),
        TileKind::Grass,
        "exactly one half is not above it"
    );

    facts.farmland = 0.6;
    assert_eq!(tiles::dry_kind(facts, RAMP), TileKind::Farmland);

    facts.forest = 0.6;
    assert_eq!(
        tiles::dry_kind(facts, RAMP),
        TileKind::Forest,
        "forest wins over farmland"
    );

    let mut high = at_fraction(MOUNTAIN_LINE);
    high.forest = 1.0;
    assert_eq!(tiles::dry_kind(high, RAMP), TileKind::Mountain);
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
        assert!(
            (tint - 1.0).abs() <= DENSITY_STRENGTH + f32::EPSILON,
            "{tint}"
        );
    }
    assert_eq!(tiles::density_tint(TileKind::Grass, dense), 1.0);
}

// A flat terrain is constructible — `ChannelMeta::linear(v, v)` — and must not divide
// by zero into a tile that is not on the strip.
#[test]
fn a_ramp_with_no_width_reads_as_flat_rather_than_as_an_abyss() {
    let flat = HeightRamp {
        low: 5.0,
        high: 5.0,
    };
    assert_eq!(flat.width(), 0.0);
    assert_eq!(tiles::classify(land(5.0), flat, f32::MAX), TileKind::Grass);
    assert_eq!(
        tiles::classify(water(1000.0), flat, f32::MAX),
        TileKind::Water
    );
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

    let mut wet = water(1.0);
    wet.dz_east = 1e6;
    assert_eq!(tiles::shade(wet, RAMP), 1.0, "standing water has no slope");
}

fn watered_terrain() -> Terrain {
    terrain_with(None)
}

fn terrain_with(forest: Option<Vec<u8>>) -> Terrain {
    let size = UVec2::new(8, 8);

    let heights: Vec<u8> = (0..64u32).map(|i| ((i / 8) * 32) as u8).collect();
    let height_layer = TerrainLayer::new(
        Some(0),
        vec![ChannelMeta::linear(0.0, 255.0)],
        LayerTexels::from_bytes(size, 1, heights).expect("8x8x1 texels"),
    );

    let mut water = Vec::with_capacity(64 * 4);
    for y in 0..8u32 {
        for x in 0..8u32 {
            water.push(if x < 2 { 255 } else { 0 });
            water.push(0);
            water.push(0);
            water.push(if x == 5 && y == 3 { 255 } else { 0 });
        }
    }
    let water_layer = TerrainLayer::new(
        Some(0),
        vec![
            ChannelMeta::linear(0.0, 255.0),
            ChannelMeta::linear(-1.0, 1.0),
            ChannelMeta::linear(-1.0, 1.0),
            ChannelMeta::linear(0.0, 1000.0),
        ],
        LayerTexels::from_bytes(size, 4, water).expect("8x8x4 texels"),
    );

    let mut fields = vec![FieldInfo {
        name: "height".to_owned(),
        role: FieldRole::Height,
        shift: 0,
        categorical: false,
        layer: 0,
        channel: 0,
    }];
    let mut layers = vec![height_layer, water_layer];
    if let Some(forest) = forest {
        layers.push(TerrainLayer::new(
            Some(0),
            vec![ChannelMeta::linear(0.0, 1.0)],
            LayerTexels::from_bytes(size, 1, forest).expect("8x8x1 texels"),
        ));
        fields.push(FieldInfo {
            name: "forest".to_owned(),
            role: FieldRole::Custom,
            shift: 0,
            categorical: false,
            layer: 2,
            channel: 0,
        });
    }

    Terrain::new(size, fields, layers, Some(WaterInfo { lakes: 1, layer: 1 }))
}

// The terrain's rows run top to bottom and a chunk's run bottom to top; getting this
// backwards puts north at the bottom of the screen and nothing else would notice.
#[test]
fn the_terrains_first_row_lands_at_the_top_of_the_chunk() {
    let terrain = watered_terrain();
    let ramp = HeightRamp::of(&terrain).expect("the fixture has a height field");
    let mut scratch = ChunkScratch::default();
    let chunk = tiles::chunk_tiles(&terrain, ramp, 0, 0, f32::MAX, &mut scratch);

    let side = tiles::CHUNK_CELLS as usize;
    let top = chunk.tiles[(side - 1) * side + 4].expect("terrain row 0 is on the terrain");
    let bottom = chunk.tiles[(side - 8) * side + 4].expect("terrain row 7 is on the terrain");

    assert_eq!(
        top.kind,
        TileKind::Grass,
        "terrain row 0 is the fixture's lowest ground"
    );
    assert_eq!(
        bottom.kind,
        TileKind::Snow,
        "the fixture's heights rise with the terrain's row index, so its highest ground \
         must come back at the chunk's lowest row — the bottom of the screen"
    );
}

// A terrain is not a whole number of chunks, so most chunks have cells that are not
// on it; those must stay undrawn rather than wrapping round.
#[test]
fn cells_off_the_terrain_are_left_undrawn() {
    let terrain = watered_terrain();
    let ramp = HeightRamp::of(&terrain).expect("height field");
    let mut scratch = ChunkScratch::default();

    let chunk = tiles::chunk_tiles(&terrain, ramp, 0, 0, f32::MAX, &mut scratch);
    let drawn = chunk.tiles.iter().filter(|tile| tile.is_some()).count();
    assert_eq!(drawn, 64, "only the terrain's 8x8 cells are drawn");

    let away = tiles::chunk_tiles(&terrain, ramp, -1, 2, f32::MAX, &mut scratch);
    assert!(
        away.tiles.iter().all(|tile| tile.is_none()),
        "a chunk entirely off the terrain draws nothing, and negative coordinates do not wrap"
    );
    assert_eq!(away.land_accumulation, None);
}

// Forest is read from a field the terrain names `forest`, not from a role, and a terrain
// without one is drawn without forest rather than refused.
#[test]
fn a_forest_field_is_read_by_name() {
    let dense: Vec<u8> = (0..64u32)
        .map(|i| if i % 8 == 4 { 255 } else { 0 })
        .collect();
    let terrain = terrain_with(Some(dense));
    let ramp = HeightRamp::of(&terrain).expect("height field");
    let mut scratch = ChunkScratch::default();
    let chunk = tiles::chunk_tiles(&terrain, ramp, 0, 0, f32::MAX, &mut scratch);

    let side = tiles::CHUNK_CELLS as usize;
    let at = |x: usize, terrain_row: usize| {
        chunk.tiles[(side - 1 - terrain_row) * side + x].expect("on the terrain")
    };
    assert_eq!(at(4, 1).kind, TileKind::Forest);
    assert_eq!(at(3, 1).kind, TileKind::Grass);
    assert_eq!(at(0, 1).kind, TileKind::Water);

    let bare = tiles::chunk_tiles(&watered_terrain(), ramp, 0, 0, f32::MAX, &mut scratch);
    assert!(
        bare.tiles
            .iter()
            .flatten()
            .all(|tile| tile.kind != TileKind::Forest)
    );
}

// A cell a settlement or a road claims is drawn as that tile and stays so whatever the
// river threshold does, while standing water under a feature is still water.
#[test]
fn a_claimed_cell_keeps_its_tile_across_threshold_moves() {
    let terrain = watered_terrain();
    let ramp = HeightRamp::of(&terrain).expect("height field");
    let mut scratch = ChunkScratch::default();
    let mut chunk = tiles::chunk_tiles(&terrain, ramp, 0, 0, f32::MAX, &mut scratch);

    let side = tiles::CHUNK_CELLS;
    let slot = |x: u32, terrain_row: u32| (side - 1 - terrain_row) * side + x;
    chunk.claim(&[
        ClaimedCell {
            slot: slot(5, 3),
            kind: TileKind::Road,
        },
        ClaimedCell {
            slot: slot(0, 3),
            kind: TileKind::City,
        },
    ]);
    chunk.apply_threshold(10.0);

    let kind = |slot: u32| chunk.tiles[slot as usize].expect("on the terrain").kind;
    assert_eq!(
        kind(slot(5, 3)),
        TileKind::Road,
        "the road crosses the channel as a bridge"
    );
    assert_eq!(kind(slot(0, 3)), TileKind::Water);
}

// The threshold is the one runtime control, and moving it must not read the terrain
// again — so what is kept beside a chunk has to be enough on its own.
#[test]
fn moving_the_threshold_rechooses_tiles_without_the_terrain() {
    let terrain = watered_terrain();
    let ramp = HeightRamp::of(&terrain).expect("height field");
    let mut scratch = ChunkScratch::default();
    let mut chunk = tiles::chunk_tiles(&terrain, ramp, 0, 0, f32::MAX, &mut scratch);

    let side = tiles::CHUNK_CELLS as usize;
    let slot = (side - 1 - 3) * side + 5;
    assert!(
        !matches!(chunk.tiles[slot].expect("on the terrain").kind, TileKind::River),
        "nothing is a river while the threshold is at the top"
    );

    assert!(chunk.affected_by(f32::MAX, 10.0));
    assert!(chunk.apply_threshold(10.0));
    assert_eq!(chunk.tiles[slot].expect("on the terrain").kind, TileKind::River);

    assert!(
        !chunk.apply_threshold(10.0),
        "re-applying the same threshold changes nothing, so the chunk is not re-uploaded"
    );
}

// Most of a map does not change when the threshold moves, and skipping those chunks is
// what makes dragging the slider affordable.
#[test]
fn a_chunk_whose_accumulation_misses_both_thresholds_is_skipped() {
    let terrain = watered_terrain();
    let ramp = HeightRamp::of(&terrain).expect("height field");
    let mut scratch = ChunkScratch::default();
    let chunk = tiles::chunk_tiles(&terrain, ramp, 0, 0, f32::MAX, &mut scratch);

    let (low, high) = chunk.land_accumulation.expect("the fixture has dry land");
    assert!(low < high, "the fixture has both quiet cells and a channel");
    assert!(!chunk.affected_by(high + 1.0, high + 2.0), "both above everything");
    assert!(chunk.affected_by(high + 1.0, low));
}

// The threshold control has to span this terrain rather than a compiled-in guess.
#[test]
fn the_accumulation_ceiling_comes_from_the_terrain() {
    let terrain = watered_terrain();
    let ceiling = tiles::accumulation_ceiling(&terrain).expect("the fixture has a water solve");
    assert!(ceiling > 900.0, "the fixture's channel reaches ~1000, got {ceiling}");

    let dry = Terrain::new(UVec2::new(4, 4), vec![], vec![], None);
    assert_eq!(tiles::accumulation_ceiling(&dry), None);
}

// A terrain with no height field is the one worth refusing, and it must not panic.
#[test]
fn a_terrain_without_a_height_field_has_no_ramp() {
    assert_eq!(HeightRamp::of(&Terrain::new(UVec2::new(4, 4), vec![], vec![], None)), None);
    let terrain = watered_terrain();
    assert!(HeightRamp::of(&terrain).is_some());
}
