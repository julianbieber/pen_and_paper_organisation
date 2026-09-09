//! Which chunks are resident, what each one currently holds, and which backdrop it was
//! filled from.
//!
//! A chunk entity is recycled rather than despawned, because its size never varies and
//! re-pointing one costs a transform and a tile rewrite where respawning would tear down
//! and rebuild a texture and a bind group on every step of a pan. Recycling across a
//! *backdrop* is the case that needs care: the tileset a chunk draws through lives on the
//! chunk, so a spare that last drew terrain has to be told about the dungeon strip before
//! it is handed a dungeon's tile indices.

use bevy::camera::Projection;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::sprite_render::{TileData, TilemapChunk, TilemapChunkTileData};
use campaign::brush::TileChange;
use campaign::tiles::{self, CHUNK_CELLS, ChunkScratch, ChunkTiles, DungeonTile, MapTile};

use crate::OpenCampaign;
use crate::document::WorldDoc;
use crate::map::backdrop::{Backdrop, BackdropSource};
use crate::map::camera::{MapCamera, viewport_of};
use crate::map::load::{DungeonTileset, MapAssets, MapTerrain};
use crate::map::panel::RiverThreshold;

/// Chunks filled per frame, so a fast pan costs frames rather than one long hitch.
pub const CHUNKS_PER_FRAME: usize = 8;

/// Where a chunk sits, in chunks.
///
/// Signed: the camera can be panned past the origin, and a chunk there is a chunk with
/// nothing on it rather than an error.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkCoord {
    pub x: i32,
    pub y: i32,
}

/// Which chunk entity is drawing which coordinate, and which are spare.
///
/// A chunk that leaves the view is kept rather than despawned, and hidden while it waits.
/// Hiding matters only on a backdrop switch — a chunk that merely left the view is off
/// screen anyway — but that is exactly when every resident chunk is given up at once while
/// only [`CHUNKS_PER_FRAME`] of them are re-pointed, so without it the terrain would stay
/// drawn over the dungeon.
#[derive(Resource, Debug, Default)]
pub struct MapChunks {
    pub live: HashMap<ChunkCoord, Entity>,
    pub free: Vec<Entity>,
    scratch: ChunkScratch,
}

/// What a chunk keeps so that a change it can answer for need not be read again.
///
/// A terrain chunk keeps what it read, so moving the river threshold re-chooses its tiles
/// without touching the terrain. A grid chunk keeps nothing: its cells are the document's,
/// a paint stroke names exactly which of them changed, and a copy here would be a second
/// answer to what the grid holds.
#[derive(Component, Debug)]
pub enum ChunkCache {
    Terrain(ChunkTiles),
    Grid,
}

/// Makes the chunks the camera can see resident, and takes back the ones it cannot.
///
/// Gives up every resident chunk when the backdrop changes under it. That is done here,
/// from the backdrop's own generation, rather than by whoever switched: the chunks are this
/// module's and a system in the authoring set reaching in to clear them would be writing a
/// map resource from the wrong half of the editor, one set too late.
pub fn stream_chunks(
    mut commands: Commands,
    open: Res<OpenCampaign>,
    backdrop: Res<Backdrop>,
    doc: Option<Res<WorldDoc>>,
    terrain: Res<MapTerrain>,
    assets: Res<MapAssets>,
    dungeon: Res<DungeonTileset>,
    threshold: Res<RiverThreshold>,
    mut chunks: ResMut<MapChunks>,
    mut drawn: Local<Option<u32>>,
    camera: Single<(&Transform, &Projection, &Camera), With<MapCamera>>,
    mut resident: Query<
        (
            &mut Transform,
            &TilemapChunk,
            &mut TilemapChunkTileData,
            &mut ChunkCoord,
            &mut ChunkCache,
            &mut Visibility,
        ),
        Without<MapCamera>,
    >,
) {
    let (camera_transform, projection, camera) = camera.into_inner();
    let Projection::Orthographic(orthographic) = projection else {
        return;
    };
    let Some(viewport) = viewport_of(camera) else {
        return;
    };

    if *drawn != Some(backdrop.generation) {
        let given_up: Vec<Entity> = chunks.live.drain().map(|(_, entity)| entity).collect();
        for entity in given_up {
            if let Ok((.., mut visibility)) = resident.get_mut(entity) {
                *visibility = Visibility::Hidden;
            }
            chunks.free.push(entity);
        }
        *drawn = Some(backdrop.generation);
    }

    let view = backdrop.view;
    let half = viewport * orthographic.scale / 2.0;
    let centre = camera_transform.translation.truncate();
    let visible = Rect::from_corners(centre - half, centre + half).inflate(view.chunk_size());
    let (columns, rows) = view.chunks_over(visible);

    let wanted: Vec<ChunkCoord> = rows
        .flat_map(|y| columns.clone().map(move |x| ChunkCoord { x, y }))
        .collect();

    let MapChunks { live, free, scratch } = &mut *chunks;
    let mut retired: Vec<Entity> = Vec::new();
    live.retain(|coord, entity| {
        let keep = wanted.contains(coord);
        if !keep {
            retired.push(*entity);
            free.push(*entity);
        }
        keep
    });
    for entity in retired {
        if let Ok((.., mut visibility)) = resident.get_mut(entity) {
            *visibility = Visibility::Hidden;
        }
    }

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

    let tileset = match backdrop.source {
        BackdropSource::Terrain => assets.tileset.clone(),
        BackdropSource::Grid => dungeon.tileset.clone(),
    };

    for coord in missing.into_iter().take(CHUNKS_PER_FRAME) {
        let (data, cache) = match backdrop.source {
            BackdropSource::Terrain => {
                let filled = tiles::chunk_tiles(
                    open.0.terrain(),
                    terrain.ramp,
                    coord.x,
                    coord.y,
                    threshold.accumulation,
                    scratch,
                );
                (to_tile_data(&filled), ChunkCache::Terrain(filled))
            }
            BackdropSource::Grid => {
                let Some(grid) = doc.as_ref().and_then(|doc| doc.document.world().grid()) else {
                    return;
                };
                (
                    to_grid_data(&tiles::grid_chunk_tiles(grid, coord.x, coord.y)),
                    ChunkCache::Grid,
                )
            }
        };
        let translation = view.chunk_translation(coord.x, coord.y);

        match free.pop() {
            Some(entity) => {
                let Ok((mut transform, chunk, mut tiles, mut at, mut held, mut visibility)) =
                    resident.get_mut(entity)
                else {
                    continue;
                };
                transform.translation = translation;
                if chunk.tileset != tileset {
                    commands.entity(entity).insert(TilemapChunk {
                        tileset: tileset.clone(),
                        ..chunk.clone()
                    });
                }
                tiles.0 = data;
                *at = coord;
                *held = cache;
                *visibility = Visibility::Inherited;
                live.insert(coord, entity);
            }
            None => {
                let entity = commands
                    .spawn((
                        TilemapChunk {
                            chunk_size: UVec2::splat(CHUNK_CELLS),
                            tile_display_size: UVec2::splat(assets.tile_size),
                            tileset: tileset.clone(),
                            alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
                        },
                        TilemapChunkTileData(data),
                        coord,
                        cache,
                        Visibility::Inherited,
                        Transform::from_translation(translation),
                    ))
                    .id();
                live.insert(coord, entity);
            }
        }
    }
}

/// Rewrites the cells a paint stroke touched, in the chunks they fall in.
///
/// Without this a painted cell reaches the screen only once its chunk has left the view and
/// come back: [`stream_chunks`] fills a chunk that is *missing*, and [`refill_chunks`]
/// answers only for the river threshold. Driven off the edit's own change list rather than
/// re-filling every resident chunk, because it runs on every stroke.
pub fn repaint_grid_chunks(
    doc: Res<WorldDoc>,
    chunks: Res<MapChunks>,
    changes: Res<PaintedCells>,
    mut resident: Query<(&ChunkCoord, &mut TilemapChunkTileData), With<ChunkCache>>,
) {
    let Some(grid) = doc.document.world().grid() else {
        return;
    };
    let side = i64::from(CHUNK_CELLS);
    let mut touched: Vec<ChunkCoord> = Vec::new();
    for change in &changes.0 {
        let coord = ChunkCoord {
            x: (i64::from(change.x).div_euclid(side)) as i32,
            y: (i64::from(change.y).div_euclid(side)) as i32,
        };
        if !touched.contains(&coord) {
            touched.push(coord);
        }
    }

    for coord in touched {
        let Some(entity) = chunks.live.get(&coord) else {
            continue;
        };
        let Ok((_, mut data)) = resident.get_mut(*entity) else {
            continue;
        };
        data.0 = to_grid_data(&tiles::grid_chunk_tiles(grid, coord.x, coord.y));
    }
}

/// The cells the last stroke changed, so the redraw need not guess which chunks moved.
///
/// Cleared by [`repaint_grid_chunks`] once it has acted, so a stroke redraws once rather
/// than every frame until the next one.
#[derive(Resource, Debug, Default)]
pub struct PaintedCells(pub Vec<TileChange>);

/// Whether a stroke is waiting to be drawn.
pub fn cells_were_painted(changes: Res<PaintedCells>) -> bool {
    !changes.0.is_empty()
}

/// Forgets the stroke once it has been drawn.
pub fn clear_painted_cells(mut changes: ResMut<PaintedCells>) {
    changes.0.clear();
}

/// Restates the resident chunks' tiles when the river threshold moves.
///
/// Reads nothing from the terrain: what a chunk kept when it was filled is enough to
/// choose between a river and the tile the cell would otherwise have had.
pub fn refill_chunks(
    threshold: Res<RiverThreshold>,
    backdrop: Res<Backdrop>,
    mut last: Local<Option<(u32, f32)>>,
    mut resident: Query<(&mut TilemapChunkTileData, &mut ChunkCache)>,
) {
    let now = threshold.accumulation;
    let before = match *last {
        Some((generation, before)) if generation == backdrop.generation => before,
        _ => now,
    };
    *last = Some((backdrop.generation, now));

    for (mut data, mut cache) in resident.iter_mut() {
        let ChunkCache::Terrain(tiles) = &mut *cache else {
            continue;
        };
        if !tiles.affected_by(before, now) {
            continue;
        }
        if tiles.apply_threshold(now) {
            data.0 = to_tile_data(tiles);
        }
    }
}

fn to_grid_data(tiles: &[Option<DungeonTile>]) -> Vec<Option<TileData>> {
    tiles
        .iter()
        .map(|tile| {
            tile.map(|tile| TileData {
                tileset_index: tile.index(),
                ..default()
            })
        })
        .collect()
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
        .insert_resource(Backdrop::terrain(64, 64, 16.0))
        .add_systems(Update, refill_chunks);
        let data = TilemapChunkTileData(to_tile_data(&chunk));
        let entity = app.world_mut().spawn((data, ChunkCache::Terrain(chunk))).id();
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
