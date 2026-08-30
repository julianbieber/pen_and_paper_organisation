//! The order the map's systems must run in.
//!
//! The ordering here is load-bearing, not tidiness. A resource inserted through
//! `Commands` becomes visible to a later system only across an explicit ordering edge,
//! because that is where bevy inserts the deferred-command flush — so the chain is
//! what lets the map appear on the frame it is built rather than the one after.

use bevy::prelude::*;

/// Where the camera is looking and how far out, within what the terrain allows.
pub mod camera;
/// Which chunks are resident, and what each one currently holds.
pub mod chunks;
/// Turning an opened campaign into a map that can be drawn, or a reason it cannot.
pub mod load;
/// The one runtime control over what counts as a river.
pub mod panel;
/// The one conversion between terrain cells and world units.
pub mod view;

use crate::OpenCampaign;
use load::{MapAssets, MapState, MapTerrain, TilesetRoot};
use panel::RiverThreshold;

/// Everything that turns an opened campaign into a map on screen.
pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RiverThreshold>()
            .init_resource::<chunks::MapChunks>()
            .insert_resource(TilesetRoot(tileset_root()))
            .add_systems(Startup, camera::spawn_camera)
            .add_systems(
                Update,
                (
                    load::open_map.run_if(
                        resource_exists::<OpenCampaign>.and_then(not(resource_exists::<MapState>)),
                    ),
                    (
                        load::watch_tileset.run_if(resource_exists::<MapAssets>),
                        load::show_map_state.run_if(resource_exists_and_changed::<MapState>),
                        panel::build_map_panel.run_if(resource_added::<MapTerrain>),
                        camera::frame_terrain.run_if(resource_exists::<MapTerrain>),
                    ),
                    panel::land_threshold,
                    camera::drive_camera
                        .run_if(resource_exists::<MapTerrain>.and_then(not(pointer_is_over_ui))),
                    chunks::stream_chunks
                        .run_if(resource_exists::<MapTerrain>.and_then(load::tileset_is_ready)),
                    chunks::refill_chunks.run_if(
                        resource_exists::<MapTerrain>.and_then(resource_changed::<RiverThreshold>),
                    ),
                )
                    .chain()
                    .before(bevy::sprite_render::update_tilemap_chunk_indices),
            );
    }
}

fn pointer_is_over_ui(hovered: Res<bevy::picking::hover::HoverMap>, nodes: Query<(), With<Node>>) -> bool {
    hovered
        .values()
        .flat_map(|hits| hits.keys())
        .any(|entity| nodes.contains(*entity))
}

fn tileset_root() -> std::path::PathBuf {
    bevy::asset::io::file::FileAssetReader::get_base_path().join("assets")
}
