//! Turning an opened campaign into a map that can be drawn, or a stated reason it
//! cannot.
//!
//! Both answers are recorded once. A refusal that is not recorded is a refusal that is
//! retried every frame for the life of the process, which is how a missing tileset
//! turns into a disk read and a leaked entity sixty times a second.

use bevy::asset::{LoadState, RenderAssetUsages};
use bevy::image::{ImageArrayLayout, ImageLoaderSettings, ImageSampler};
use bevy::prelude::*;
use campaign::atlas::{self, AtlasMeta};
use campaign::style;
use campaign::tiles::{self, COMBAT_TILE_COUNT, DUNGEON_TILE_COUNT, DungeonTile, HeightRamp};

use crate::OpenCampaign;
use crate::StatusMessage;
use crate::map::backdrop::{Backdrop, RetiredBackdrop};

/// The tileset shipped with the tool, relative to the assets directory.
///
/// One strip for every campaign: the backdrop is deliberately neutral, so there is
/// nothing per-campaign about it. Edit it in place with `bevy_sprite_editor`.
pub const TILESET_BASE: &str = "terrain_tiles";

/// The combat tileset shipped with the tool, relative to the assets directory.
///
/// Drawn by hand in `bevy_sprite_editor` and loaded exactly as [`TILESET_BASE`] is; its
/// columns are [`CombatTile`](campaign::CombatTile)'s variants in order.
pub const COMBAT_TILESET_BASE: &str = "combat_tiles";

/// Whether there is a map, decided once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapOutcome {
    Ready,
    Unavailable,
}

/// What became of the attempt to build a map, and what to say about it.
///
/// Its presence is what stops [`open_map`] running again, so it is inserted on the
/// refusing paths as much as on the succeeding one.
#[derive(Resource, Debug, Clone)]
pub struct MapState {
    pub outcome: MapOutcome,
    pub message: String,
}

impl MapState {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            outcome: MapOutcome::Unavailable,
            message: message.into(),
        }
    }
}

/// The tileset a dungeon's grid is drawn through, built rather than loaded.
///
/// One flat-coloured layer per [`DungeonTile`], so a tile's index is its layer exactly as
/// it is for the terrain. It is generated because no hand-drawn dungeon strip exists and
/// none of this issue's acceptance criteria is about how a floor tile looks — nine solid
/// colours at the terrain's own tile size cost nine kilobytes and let a dungeon reuse the
/// whole tilemap path. The colours are a decision like every other colour on the map, so
/// they live in [`campaign::style`]; only the image is built here.
///
/// The layers are stacked vertically and a tile's layer is its [`DungeonTile::index`],
/// which is what the tilemap material is handed.
///
/// The four door and stair kinds differ by hue alone. That is deliberate rather than
/// unfinished: swapping in a hand-drawn strip later replaces the one function that builds
/// this and leaves every other decision in this workspace alone.
#[derive(Resource, Debug, Clone)]
pub struct DungeonTileset {
    pub tileset: Handle<Image>,
}

/// The strip a combat map is drawn through, or why there is none.
///
/// A missing or unusable combat strip refuses a new combat map rather than the whole map,
/// which does not depend on it.
#[derive(Resource, Debug, Clone)]
pub struct CombatTileset {
    pub tileset: Result<Handle<Image>, String>,
}

/// The tileset, once it has been asked for.
#[derive(Resource, Debug, Clone)]
pub struct MapAssets {
    pub tileset: Handle<Image>,
    /// Pixels on a tile's side, which is also how many world units a terrain cell
    /// spans.
    pub tile_size: u32,
}

/// What the map needs to know about the terrain, read once so no system re-reads it
/// per cell.
#[derive(Resource, Debug, Clone, Copy)]
pub struct MapTerrain {
    pub width: u32,
    pub height: u32,
    pub ramp: HeightRamp,
    /// The highest accumulation the terrain reaches, which is what the threshold
    /// control spans. Zero on a terrain with no water solve.
    pub accumulation_high: f32,
}

/// Reads the tileset sidecars, asks for the terrain and combat strips as array textures,
/// builds the dungeon strip, and describes the terrain and the backdrop it makes — exactly
/// once per opened campaign. A combat strip that cannot be read is recorded on
/// [`CombatTileset`] and does not make the map unavailable.
pub fn open_map(
    mut commands: Commands,
    open: Res<OpenCampaign>,
    assets: Res<AssetServer>,
    tileset_root: Res<TilesetRoot>,
    mut images: ResMut<Assets<Image>>,
    retired: Option<Res<RetiredBackdrop>>,
) {
    let base = tileset_root.0.join(TILESET_BASE);
    let meta = match terrain_meta(&tileset_root.0) {
        Ok(meta) => meta,
        Err(error) => {
            commands.insert_resource(MapState::unavailable(error.to_string()));
            return;
        }
    };
    if !meta.is_known_version() {
        warn!(
            "{} declares tileset format version {}; reading it as {}",
            atlas::sidecar_path(&base).display(),
            meta.version,
            atlas::SIDECAR_VERSION
        );
    }

    let terrain = open.0.terrain();
    let Some(ramp) = HeightRamp::of(terrain) else {
        commands.insert_resource(MapState::unavailable(format!(
            "`{}` has no height field, or no extent: there is nothing to draw a map from",
            open.0.terrain_dir().display()
        )));
        return;
    };

    let tileset = load_strip(&assets, format!("{TILESET_BASE}.png"), meta.width_in_tiles);

    let combat_base = tileset_root.0.join(COMBAT_TILESET_BASE);
    let combat = match combat_meta(&tileset_root.0) {
        Ok(combat) => {
            if !combat.is_known_version() {
                warn!(
                    "{} declares tileset format version {}; reading it as {}",
                    atlas::sidecar_path(&combat_base).display(),
                    combat.version,
                    atlas::SIDECAR_VERSION
                );
            }
            Ok(load_strip(
                &assets,
                format!("{COMBAT_TILESET_BASE}.png"),
                combat.width_in_tiles,
            ))
        }
        Err(error) => {
            warn!("{error}");
            Err(error.to_string())
        }
    };
    commands.insert_resource(CombatTileset { tileset: combat });

    commands.insert_resource(DungeonTileset {
        tileset: images.add(build_dungeon_tileset(meta.tile_size)),
    });
    commands.insert_resource(MapAssets {
        tileset,
        tile_size: meta.tile_size,
    });
    commands.insert_resource(MapTerrain {
        width: terrain.width(),
        height: terrain.height(),
        ramp,
        accumulation_high: tiles::accumulation_ceiling(terrain).unwrap_or(0.0),
    });
    commands.insert_resource(Backdrop::terrain_after(
        terrain.width(),
        terrain.height(),
        meta.tile_size as f32,
        retired.map(|retired| retired.0),
    ));
    commands.insert_resource(MapState {
        outcome: MapOutcome::Ready,
        message: String::new(),
    });
}

fn load_strip(assets: &AssetServer, file: String, columns: u32) -> Handle<Image> {
    assets
        .load_builder()
        .with_settings(move |settings: &mut ImageLoaderSettings| {
            settings.array_layout = Some(ImageArrayLayout::GridCount { columns, rows: 1 });
            settings.sampler = ImageSampler::nearest();
            settings.asset_usage = RenderAssetUsages::RENDER_WORLD;
        })
        .load(file)
}

fn build_dungeon_tileset(tile_size: u32) -> Image {
    let side = tile_size.max(1);
    let layers = u32::from(DUNGEON_TILE_COUNT);
    let mut pixels = vec![0u8; (side * side * layers * 4) as usize];

    for tile in DungeonTile::all() {
        let [red, green, blue] = style::dungeon_tile(tile);
        let bytes = [channel(red), channel(green), channel(blue), 255];
        let top = u32::from(tile.index()) * side;
        for row in top..top + side {
            for column in 0..side {
                let at = (((row * side) + column) * 4) as usize;
                pixels[at..at + 4].copy_from_slice(&bytes);
            }
        }
    }

    let mut image = Image::new(
        bevy::render::render_resource::Extent3d {
            width: side,
            height: side * layers,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        pixels,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
        .reinterpret_stacked_2d_as_array(layers)
        .expect("the image is `layers` whole tiles tall, so it splits into that many");
    image
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn terrain_meta(root: &std::path::Path) -> Result<AtlasMeta, atlas::AtlasError> {
    AtlasMeta::read(&root.join(TILESET_BASE), tiles::TILE_COUNT)
}

fn combat_meta(root: &std::path::Path) -> Result<AtlasMeta, atlas::AtlasError> {
    AtlasMeta::read(&root.join(COMBAT_TILESET_BASE), COMBAT_TILE_COUNT)
}

/// Why a combat map cannot be drawn through `tileset` yet, or `None` when it can.
///
/// The sidecar's refusal, the asset server's load failure, or that the strip is still
/// loading.
pub fn combat_tileset_refusal(assets: &AssetServer, tileset: &CombatTileset) -> Option<String> {
    let handle = match &tileset.tileset {
        Ok(handle) => handle,
        Err(error) => return Some(format!("the combat tileset cannot be used: {error}")),
    };
    if let Some(LoadState::Failed(error)) = assets.get_load_state(handle) {
        return Some(format!("the combat tileset could not be loaded: {error}"));
    }
    if !assets.is_loaded_with_dependencies(handle) {
        return Some("the combat tileset is still loading".to_owned());
    }
    None
}

/// Where the tileset's files sit on disk, so the sidecar can be read beside the image
/// the asset server loads.
#[derive(Resource, Debug, Clone)]
pub struct TilesetRoot(pub std::path::PathBuf);

/// Turns a tileset the asset server could not load into a stated reason.
///
/// Without this a strip whose width is not a whole number of tiles leaves the map
/// blank and silent, because the only symptom is a handle that never becomes loaded.
pub fn watch_tileset(
    assets: Res<AssetServer>,
    map_assets: Res<MapAssets>,
    mut state: ResMut<MapState>,
) {
    if state.outcome == MapOutcome::Unavailable {
        return;
    }
    if let Some(LoadState::Failed(error)) = assets.get_load_state(&map_assets.tileset) {
        *state = MapState::unavailable(format!("the tileset could not be loaded: {error}"));
    }
}

/// Puts whatever [`MapState`] says on the status line the dialog also writes to.
pub fn show_map_state(state: Res<MapState>, mut status: ResMut<StatusMessage>) {
    if status.0 != state.message {
        status.0.clone_from(&state.message);
    }
}

/// Whether the tileset has finished loading, which is what the chunks wait on.
pub fn tileset_is_ready(assets: Option<Res<AssetServer>>, map: Option<Res<MapAssets>>) -> bool {
    let (Some(assets), Some(map)) = (assets, map) else {
        return false;
    };
    assets.is_loaded_with_dependencies(&map.tileset)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The terrain strip is still checked against TILE_COUNT now the reader is handed a count.
    #[test]
    fn the_committed_terrain_strip_is_read_against_the_terrain_tile_count() {
        let meta = terrain_meta(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")))
            .expect("the committed terrain strip reads");
        assert!(meta.width_in_tiles >= u32::from(tiles::TILE_COUNT));
    }

    // The combat strip is art that gets redrawn: this pins that what is committed still
    // reads against the vocabulary's count, and that the PNG is the size its sidecar says.
    #[test]
    fn the_committed_combat_strip_is_read_against_the_combat_tile_count() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"));
        let meta = combat_meta(root).expect("the committed combat strip reads");
        assert!(meta.width_in_tiles >= u32::from(COMBAT_TILE_COUNT));

        let png = std::fs::read(root.join(format!("{COMBAT_TILESET_BASE}.png"))).expect("the strip is committed");
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert_eq!(width, meta.width_in_tiles * meta.tile_size);
        assert_eq!(height, meta.height_in_tiles * meta.tile_size);
    }
}
