//! What the map decides about a cell, checked without a window.
//!
//! Most of it needs no terrain at all: the classifier is a function of a `CellFacts`
//! and a `HeightRamp`, so the awkward cases are stated directly rather than baked into
//! a fixture and hoped for.

use campaign::tiles::{
    self, CellFacts, ChunkScratch, HeightRamp, LAND_BANDS, MapTile, SHADE_STRENGTH, TILE_COUNT,
    TileKind,
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
        water_mask: 0,
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

// A coast is only reached with a wet neighbour, so mask zero must not consume a
// column — the bug that made the strip one tile longer than it could ever draw.
#[test]
fn the_coast_columns_start_at_the_first_mask_that_can_occur() {
    assert_eq!(TileKind::Coast(1).index(), TileKind::River.index() + 1);
    assert_eq!(TileKind::Coast(15).index(), TILE_COUNT - 1);
}

// Water beats everything, whatever the threshold says about the water's accumulation.
#[test]
fn standing_water_is_never_a_river() {
    let mut facts = water(1.0);
    facts.accumulation = f32::MAX;
    assert_eq!(tiles::classify(facts, RAMP, 0.0), TileKind::ShallowWater);
}

// Depth splits at one band, and the split is stated against the ramp rather than a
// fixed depth.
#[test]
fn water_deeper_than_one_band_is_drawn_deep() {
    let band = RAMP.band_height();
    assert_eq!(
        tiles::classify(water(band * 0.5), RAMP, 0.0),
        TileKind::ShallowWater
    );
    assert_eq!(
        tiles::classify(water(band), RAMP, 0.0),
        TileKind::ShallowWater,
        "exactly one band deep is not deeper than one band"
    );
    assert_eq!(
        tiles::classify(water(band * 1.5), RAMP, 0.0),
        TileKind::DeepWater
    );
}

// `watershed` reports zero accumulation off the edge of the terrain and its own
// channel test is `>=`, so a threshold of zero must not turn the whole map into river.
#[test]
fn a_threshold_of_zero_does_not_make_every_cell_a_river() {
    assert_eq!(
        tiles::classify(land(10.0), RAMP, 0.0),
        TileKind::Land(1),
        "a dry cell with no accumulation is not a channel at threshold zero"
    );

    let mut flowing = land(10.0);
    flowing.accumulation = 0.5;
    assert_eq!(tiles::classify(flowing, RAMP, 0.0), TileKind::River);
}

// A river reaching the sea is a mouth; drawing it as coast would break the drainage.
#[test]
fn a_channel_beats_a_coastline() {
    let mut facts = land(10.0);
    facts.accumulation = 100.0;
    facts.water_mask = 0b0100;
    assert_eq!(tiles::classify(facts, RAMP, 10.0), TileKind::River);
    assert_eq!(tiles::classify(facts, RAMP, 1000.0), TileKind::Coast(0b0100));
}

// Every neighbour arrangement has a tile, and none of them lands off the strip.
#[test]
fn every_neighbour_mask_draws_a_coast_on_the_strip() {
    for mask in 1..=15u8 {
        let mut facts = land(10.0);
        facts.water_mask = mask;
        let kind = tiles::classify(facts, RAMP, f32::MAX);
        assert_eq!(kind, TileKind::Coast(mask));
        assert!(kind.index() < TILE_COUNT);
    }
}

// A terrain with no water solve reaches only the band arm, and that is not an error.
#[test]
fn a_dry_terrain_is_all_bands() {
    for step in 0..=LAND_BANDS {
        let height = RAMP.low + RAMP.width() * f32::from(step) / f32::from(LAND_BANDS);
        let kind = tiles::classify(land(height), RAMP, f32::MAX);
        assert!(matches!(kind, TileKind::Land(_)), "{height} gave {kind:?}");
        assert!(kind.index() < TILE_COUNT);
    }
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
    assert_eq!(tiles::classify(land(5.0), flat, f32::MAX), TileKind::Land(0));
    assert_eq!(
        tiles::classify(water(1000.0), flat, f32::MAX),
        TileKind::ShallowWater,
        "with no ramp to measure against, water is shallow rather than an abyss"
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
    assert_eq!(inverted.band(30.0), 0);

    for height in [f32::NAN, f32::INFINITY, -f32::INFINITY, -1e9, 1e9] {
        let band = RAMP.band(height);
        assert!(band < LAND_BANDS, "{height} gave band {band}");
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

    let field = FieldInfo {
        name: "height".to_owned(),
        role: FieldRole::Height,
        shift: 0,
        categorical: false,
        layer: 0,
        channel: 0,
    };

    Terrain::new(
        size,
        vec![field],
        vec![height_layer, water_layer],
        Some(WaterInfo { lakes: 1, layer: 1 }),
    )
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

    let band = |tile: MapTile| match tile.kind {
        TileKind::Land(band) => band,
        other => panic!("column 4 is dry land, got {other:?}"),
    };
    assert!(
        band(top) < band(bottom),
        "the fixture's heights rise with the terrain's row index, so the lowest band \
         must come back at the chunk's highest row — the top of the screen"
    );
    assert_eq!(band(top), 0, "terrain row 0 is the fixture's lowest ground");
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

// The coastline is read from the terrain's own neighbours, and the edge of the map is
// not one of them.
#[test]
fn the_terrains_border_is_not_drawn_as_a_coastline() {
    let terrain = watered_terrain();
    let ramp = HeightRamp::of(&terrain).expect("height field");
    let mut scratch = ChunkScratch::default();
    let chunk = tiles::chunk_tiles(&terrain, ramp, 0, 0, f32::MAX, &mut scratch);

    let side = tiles::CHUNK_CELLS as usize;
    let at = |x: usize, terrain_row: usize| {
        chunk.tiles[(side - 1 - terrain_row) * side + x].expect("on the terrain")
    };

    assert_eq!(
        at(2, 4).kind,
        TileKind::Coast(0b1000),
        "the first dry column has water to its west and land everywhere else"
    );
    assert!(
        matches!(at(7, 4).kind, TileKind::Land(_)),
        "the eastern edge has no wet neighbour: off the terrain counts as land, not coast"
    );
    assert!(matches!(at(0, 0).kind, TileKind::ShallowWater | TileKind::DeepWater));
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
