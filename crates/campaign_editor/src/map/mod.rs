//! The order the map's systems must run in.
//!
//! The ordering here is load-bearing, not tidiness. A resource inserted through
//! `Commands` becomes visible to a later system only across an explicit ordering edge,
//! because that is where bevy inserts the deferred-command flush — so the chain is
//! what lets the map appear on the frame it is built rather than the one after.

use bevy::prelude::*;

/// What the open document is drawn on, and where the camera should be looking at it.
pub mod backdrop;
/// Where the camera is looking and how far out, within what the live backdrop allows.
pub mod camera;
/// Which chunks are resident, and what each one currently holds.
pub mod chunks;
/// Reading the picture a document declares, and drawing it under the features.
pub mod image;
/// Turning an opened campaign into a map that can be drawn, or a reason it cannot.
pub mod load;
/// The one runtime control over what counts as a river.
pub mod panel;
/// Where the cursor is, in terrain cells.
pub mod pointer;
/// What one cell is worth and how fast the party travels, as the GM sets them.
pub mod scale;
/// The bar saying how far a stretch of screen is.
pub mod scalebar;
/// The one conversion between terrain cells and world units.
pub mod view;

use crate::{EditorSet, OpenCampaign};
use backdrop::Backdrop;
use load::{MapAssets, MapState, MapTerrain, TilesetRoot};
use panel::RiverThreshold;
use pointer::{MapPointer, PointerOverride};

/// Everything that turns an opened campaign into a map on screen.
pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RiverThreshold>()
            .init_resource::<chunks::MapChunks>()
            .init_resource::<chunks::PaintedCells>()
            .init_resource::<MapPointer>()
            .init_resource::<image::ImageAsset>()
            .init_resource::<PointerOverride>()
            .init_resource::<scale::ScaleFields>()
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
                        panel::show_map_panel.run_if(resource_exists_and_changed::<Backdrop>),
                        scalebar::build_scale_bar.run_if(resource_added::<MapTerrain>),
                        scale::build_scale_panel.run_if(
                            resource_added::<MapTerrain>.and_then(resource_exists::<OpenCampaign>),
                        ),
                        camera::place_camera.run_if(resource_exists::<Backdrop>),
                    ),
                    panel::land_threshold,
                    scale::land_speed,
                    (scale::commit_scale, scale::land_campaign_scale)
                        .chain()
                        .run_if(resource_exists::<OpenCampaign>),
                    camera::drive_camera.run_if(
                        resource_exists::<Backdrop>
                            .and_then(not(pointer_is_over_ui))
                            .and_then(not(crate::features::prompt::a_question_is_up)),
                    ),
                    pointer::track_pointer.run_if(resource_exists::<Backdrop>),
                    scalebar::show_scale_bar.run_if(
                        resource_exists::<Backdrop>.and_then(resource_exists::<OpenCampaign>),
                    ),
                    chunks::stream_chunks
                        .run_if(resource_exists::<Backdrop>.and_then(load::tileset_is_ready)),
                    image::sync_backdrop_image.run_if(image::a_document_is_open),
                    chunks::refill_chunks.run_if(
                        resource_changed::<RiverThreshold>
                            .and_then(backdrop::backdrop_is_the_terrain),
                    ),
                    (chunks::repaint_grid_chunks, chunks::clear_painted_cells)
                        .chain()
                        .run_if(
                            chunks::cells_were_painted
                                .and_then(resource_exists::<crate::document::WorldDoc>),
                        ),
                )
                    .chain()
                    .in_set(EditorSet::Map)
                    .before(bevy::sprite_render::update_tilemap_chunk_indices),
            );
    }
}

/// Whether the pointer is over any UI node.
///
/// Public because authoring needs the same answer: a second implementation of "is the
/// pointer over the UI" is a second answer to it, and the two would disagree the first
/// time a widget changed.
///
/// This is a *hover* test, rebuilt every frame with no notion of a drag having been
/// captured — so it is the right question to ask of a press, and the wrong one to ask of
/// a drag already in flight.
pub fn pointer_is_over_ui(
    hovered: Res<bevy::picking::hover::HoverMap>,
    nodes: Query<(), With<Node>>,
) -> bool {
    hovered
        .values()
        .flat_map(|hits| hits.keys())
        .any(|entity| nodes.contains(*entity))
}

fn tileset_root() -> std::path::PathBuf {
    bevy::asset::io::file::FileAssetReader::get_base_path().join("assets")
}
