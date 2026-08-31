//! Which chunks are resident, and what each one currently holds.

use bevy::camera::Projection;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::sprite_render::{TileData, TilemapChunk, TilemapChunkTileData};
use campaign::tiles::{self, CHUNK_CELLS, ChunkScratch, ChunkTiles, MapTile};

use crate::OpenCampaign;
use crate::map::camera::{MapCamera, viewport_of};
use crate::map::load::{MapAssets, MapTerrain};
use crate::map::panel::RiverThreshold;
use crate::map::view::MapView;

/// Chunks filled per frame, so a fast pan costs frames rather than one long hitch.
pub const CHUNKS_PER_FRAME: usize = 8;

/// Where a chunk sits, in chunks.
///
/// Signed: the camera can be panned past the terrain's origin, and a chunk there is a
/// chunk with nothing on it rather than an error.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkCoord {
    pub x: i32,
    pub y: i32,
}

/// Which chunk entity is drawing which coordinate, and which are spare.
///
/// A chunk that leaves the view is kept rather than despawned: its size and its
/// tileset never vary, so re-pointing one costs a transform and a tile rewrite, where
/// respawning would tear down and rebuild a texture and a bind group on every step of
/// a pan.
#[derive(Resource, Debug, Default)]
pub struct MapChunks {
    pub live: HashMap<ChunkCoord, Entity>,
    pub free: Vec<Entity>,
    scratch: ChunkScratch,
}

/// What a chunk keeps so a threshold move need not read the terrain again.
#[derive(Component, Debug)]
pub struct ChunkCache(ChunkTiles);

/// Makes the chunks the camera can see resident, and takes back the ones it cannot.
pub fn stream_chunks(
    mut commands: Commands,
    open: Res<OpenCampaign>,
    terrain: Res<MapTerrain>,
    assets: Res<MapAssets>,
    threshold: Res<RiverThreshold>,
    mut chunks: ResMut<MapChunks>,
    camera: Single<(&Transform, &Projection, &Camera), With<MapCamera>>,
    mut resident: Query<(&mut Transform, &mut TilemapChunkTileData, &mut ChunkCoord, &mut ChunkCache), Without<MapCamera>>,
) {
    let (camera_transform, projection, camera) = camera.into_inner();
    let Projection::Orthographic(orthographic) = projection else {
        return;
    };
    let Some(viewport) = viewport_of(camera) else {
        return;
    };

    let view = MapView::new(terrain.width, terrain.height, assets.tile_size as f32);
    let half = viewport * orthographic.scale / 2.0;
    let centre = camera_transform.translation.truncate();
    let visible = Rect::from_corners(centre - half, centre + half).inflate(view.chunk_size());
    let (columns, rows) = view.chunks_over(visible);

    let wanted: Vec<ChunkCoord> = rows
        .flat_map(|y| columns.clone().map(move |x| ChunkCoord { x, y }))
        .collect();

    let MapChunks { live, free, scratch } = &mut *chunks;
    live.retain(|coord, entity| {
        let keep = wanted.contains(coord);
        if !keep {
            free.push(*entity);
        }
        keep
    });

    let mut missing: Vec<ChunkCoord> = wanted
        .into_iter()
        .filter(|coord| !live.contains_key(coord))
        .collect();
    let middle = view.world_to_cell(centre) / CHUNK_CELLS as f32;
    missing.sort_by(|a, b| {
        let distance = |coord: &ChunkCoord| {
            let dx = coord.x as f32 - middle.x;
            let dy = coord.y as f32 - middle.y;
            dx * dx + dy * dy
        };
        distance(a).total_cmp(&distance(b))
    });

    for coord in missing.into_iter().take(CHUNKS_PER_FRAME) {
        let filled = tiles::chunk_tiles(
            open.0.terrain(),
            terrain.ramp,
            coord.x,
            coord.y,
            threshold.accumulation,
            scratch,
        );
        let translation = view.chunk_translation(coord.x, coord.y);

        match free.pop() {
            Some(entity) => {
                let Ok((mut transform, mut data, mut at, mut cache)) = resident.get_mut(entity)
                else {
                    continue;
                };
                transform.translation = translation;
                data.0 = to_tile_data(&filled);
                *at = coord;
                cache.0 = filled;
                live.insert(coord, entity);
            }
            None => {
                let entity = commands
                    .spawn((
                        TilemapChunk {
                            chunk_size: UVec2::splat(CHUNK_CELLS),
                            tile_display_size: UVec2::splat(assets.tile_size),
                            tileset: assets.tileset.clone(),
                            alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
                        },
                        TilemapChunkTileData(to_tile_data(&filled)),
                        coord,
                        ChunkCache(filled),
                        Transform::from_translation(translation),
                    ))
                    .id();
                live.insert(coord, entity);
            }
        }
    }
}

/// Restates the resident chunks' tiles when the river threshold moves.
///
/// Reads nothing from the terrain: what a chunk kept when it was filled is enough to
/// choose between a river and the tile the cell would otherwise have had.
pub fn refill_chunks(
    threshold: Res<RiverThreshold>,
    mut last: Local<Option<f32>>,
    mut resident: Query<(&mut TilemapChunkTileData, &mut ChunkCache)>,
) {
    let now = threshold.accumulation;
    let before = last.unwrap_or(now);
    *last = Some(now);

    for (mut data, mut cache) in resident.iter_mut() {
        if !cache.0.affected_by(before, now) {
            continue;
        }
        if cache.0.apply_threshold(now) {
            data.0 = to_tile_data(&cache.0);
        }
    }
}

fn to_tile_data(chunk: &ChunkTiles) -> Vec<Option<TileData>> {
    chunk
        .tiles
        .iter()
        .map(|tile| {
            tile.map(|MapTile { kind, shade }| TileData {
                tileset_index: kind.index(),
                color: Color::srgb(shade, shade, shade),
                ..default()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use campaign::tiles::{CellCache, TileKind};

    fn chunk_with(accumulations: &[f32]) -> ChunkTiles {
        let cells: Vec<Option<CellCache>> = accumulations
            .iter()
            .map(|accumulation| {
                Some(CellCache {
                    dry: TileKind::Land(2),
                    shade: 1.0,
                    river_accumulation: Some(*accumulation),
                })
            })
            .collect();
        let mut chunk = ChunkTiles {
            tiles: vec![None; cells.len()],
            cells,
            land_accumulation: Some((
                accumulations.iter().copied().fold(f32::MAX, f32::min),
                accumulations.iter().copied().fold(f32::MIN, f32::max),
            )),
        };
        chunk.apply_threshold(f32::MAX);
        chunk
    }

    fn app_with(chunk: ChunkTiles) -> (App, Entity) {
        let mut app = App::new();
        app.insert_resource(RiverThreshold {
            accumulation: f32::MAX,
        })
        .add_systems(Update, refill_chunks);
        let data = TilemapChunkTileData(to_tile_data(&chunk));
        let entity = app.world_mut().spawn((data, ChunkCache(chunk))).id();
        (app, entity)
    }

    fn rivers(app: &App, entity: Entity) -> usize {
        app.world()
            .entity(entity)
            .get::<TilemapChunkTileData>()
            .expect("the chunk keeps its tiles")
            .0
            .iter()
            .filter(|tile| {
                tile.is_some_and(|tile| tile.tileset_index == TileKind::River.index())
            })
            .count()
    }

    // The acceptance criterion the UI cannot be driven headlessly for: moving the
    // threshold changes which cells are drawn as channels, without respawning a chunk.
    #[test]
    fn lowering_the_threshold_turns_more_cells_into_rivers() {
        let (mut app, entity) = app_with(chunk_with(&[10.0, 100.0, 1000.0, 5000.0]));
        app.update();
        assert_eq!(rivers(&app, entity), 0, "nothing is a river at the top");

        app.world_mut().resource_mut::<RiverThreshold>().accumulation = 500.0;
        app.update();
        assert_eq!(rivers(&app, entity), 2, "the two busiest cells become channels");

        app.world_mut().resource_mut::<RiverThreshold>().accumulation = 50.0;
        app.update();
        assert_eq!(rivers(&app, entity), 3);
    }

    // Most of a map does not change when the threshold moves, and skipping those
    // chunks is what makes dragging the slider affordable at all.
    #[test]
    fn a_chunk_the_threshold_cannot_reach_is_left_alone() {
        let (mut app, entity) = app_with(chunk_with(&[1.0, 2.0, 3.0, 4.0]));
        app.update();

        app.world_mut().resource_mut::<RiverThreshold>().accumulation = 900.0;
        app.update();
        assert_eq!(rivers(&app, entity), 0);

        app.world_mut().resource_mut::<RiverThreshold>().accumulation = 800.0;
        app.update();
        assert_eq!(rivers(&app, entity), 0, "still nothing, and nothing re-uploaded");
    }
}
