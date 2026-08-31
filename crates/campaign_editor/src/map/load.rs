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
use campaign::tiles::{self, HeightRamp};

use crate::OpenCampaign;
use crate::StatusMessage;

/// The tileset shipped with the tool, relative to the assets directory.
///
/// One strip for every campaign: the backdrop is deliberately neutral, so there is
/// nothing per-campaign about it. Edit it in place with `bevy_sprite_editor`.
pub const TILESET_BASE: &str = "terrain_tiles";

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

/// Reads the tileset sidecar, asks for its strip as an array texture, and describes
/// the terrain — exactly once per opened campaign.
pub fn open_map(
    mut commands: Commands,
    open: Res<OpenCampaign>,
    assets: Res<AssetServer>,
    tileset_root: Res<TilesetRoot>,
) {
    let base = tileset_root.0.join(TILESET_BASE);
    let meta = match AtlasMeta::read(&base) {
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

    let columns = meta.width_in_tiles;
    let tileset = assets
        .load_builder()
        .with_settings(move |settings: &mut ImageLoaderSettings| {
            settings.array_layout = Some(ImageArrayLayout::GridCount { columns, rows: 1 });
            settings.sampler = ImageSampler::nearest();
            settings.asset_usage = RenderAssetUsages::RENDER_WORLD;
        })
        .load(format!("{TILESET_BASE}.png"));

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
    commands.insert_resource(MapState {
        outcome: MapOutcome::Ready,
        message: String::new(),
    });
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
