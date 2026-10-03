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
use campaign::claims::Claims;
use campaign::grid::{TileGrid, TileVocabulary};
use campaign::tiles::{self, CHUNK_CELLS, ChunkScratch, ChunkTiles, MapTile};

use crate::OpenCampaign;
use crate::combat::CombatMaps;
use crate::document::WorldDoc;
use crate::map::backdrop::{Backdrop, BackdropSource};
use crate::map::camera::{MapCamera, viewport_of};
use crate::map::load::{CombatTileset, DungeonTileset, MapAssets, MapTerrain};

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

/// Which backdrop a chunk was filled from.
///
/// Neither keeps a copy of its cells: a terrain chunk is filled again from the terrain
/// when a claim over it changes, and a grid chunk's cells are the document's.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkCache {
    Terrain,
    Grid,
}

/// The world map's settlements and roads, as the terrain cells they draw over.
///
/// Kept from the last world map on screen: a dungeon's edits cannot move a settlement,
/// so it is neither cleared nor rebuilt while one is.
#[derive(Resource, Debug, Default)]
pub struct FeatureClaims(pub Claims);

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
    combat: Option<Res<CombatMaps>>,
    terrain: Res<MapTerrain>,
    assets: Res<MapAssets>,
    dungeon: Res<DungeonTileset>,
    combat_tileset: Option<Res<CombatTileset>>,
    claims: Res<FeatureClaims>,
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
        BackdropSource::Combat => match combat_tileset.as_ref().map(|combat| &combat.tileset) {
            Some(Ok(handle)) => handle.clone(),
            _ => return,
        },
    };

    for coord in missing.into_iter().take(CHUNKS_PER_FRAME) {
        let (data, cache) = match backdrop.source {
            BackdropSource::Terrain => {
                let mut filled = tiles::chunk_tiles(
                    open.0.terrain(),
                    terrain.ramp,
                    coord.x,
                    coord.y,
                    scratch,
                );
                filled.claim(claims.0.in_chunk(coord.x, coord.y));
                (to_tile_data(&filled), ChunkCache::Terrain)
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
            BackdropSource::Combat => {
                let Some(map) = combat.as_ref().and_then(|combat| combat.on_screen()) else {
                    return;
                };
                (
                    to_grid_data(&tiles::grid_chunk_tiles(map.content().grid(), coord.x, coord.y)),
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

/// Redraws the resident terrain chunks a world map edit moved a settlement or a road
/// across.
///
/// Rebuilds the claims whenever the document changes and redraws only the chunks whose
/// claimed cells differ, so an edit that moves nothing claimed redraws nothing. A dungeon
/// on screen is left alone.
pub fn reclaim_cells(
    open: Res<OpenCampaign>,
    doc: Res<WorldDoc>,
    terrain: Res<MapTerrain>,
    mut claims: ResMut<FeatureClaims>,
    mut chunks: ResMut<MapChunks>,
    mut resident: Query<(&mut TilemapChunkTileData, &mut ChunkCache)>,
) {
    let world = doc.document.world();
    if world.grid().is_some() {
        return;
    }
    let fresh = Claims::of(world, terrain.width, terrain.height);
    let differing = fresh.chunks_differing_from(&claims.0);
    if differing.is_empty() {
        return;
    }
    claims.0 = fresh;

    let MapChunks { live, scratch, .. } = &mut *chunks;
    for (x, y) in differing {
        let Some(entity) = live.get(&ChunkCoord { x, y }) else {
            continue;
        };
        let Ok((mut data, cache)) = resident.get_mut(*entity) else {
            continue;
        };
        if *cache != ChunkCache::Terrain {
            continue;
        }
        let mut filled = tiles::chunk_tiles(
            open.0.terrain(),
            terrain.ramp,
            x,
            y,
            scratch,
        );
        filled.claim(claims.0.in_chunk(x, y));
        data.0 = to_tile_data(&filled);
    }
}

/// Rewrites the cells a paint stroke touched, in the chunks they fall in, from the grid of
/// whichever document the backdrop is.
///
/// Without this a painted cell reaches the screen only once its chunk has left the view and
/// come back: [`stream_chunks`] fills a chunk that is *missing*. A stroke rewrites only the chunks its cells fall
/// in; an undo or a redo rewrites every resident chunk. Does nothing on the terrain.
pub fn repaint_grid_chunks(
    backdrop: Res<Backdrop>,
    doc: Option<Res<WorldDoc>>,
    combat: Option<Res<CombatMaps>>,
    chunks: Res<MapChunks>,
    changes: Res<PaintedCells>,
    mut resident: Query<(&ChunkCoord, &mut TilemapChunkTileData), With<ChunkCache>>,
) {
    let side = i64::from(CHUNK_CELLS);
    let touched: Vec<ChunkCoord> = if changes.everything {
        chunks.live.keys().copied().collect()
    } else {
        let mut touched = Vec::new();
        for (x, y) in &changes.cells {
            let coord = ChunkCoord {
                x: (i64::from(*x).div_euclid(side)) as i32,
                y: (i64::from(*y).div_euclid(side)) as i32,
            };
            if !touched.contains(&coord) {
                touched.push(coord);
            }
        }
        touched
    };

    match backdrop.source {
        BackdropSource::Terrain => {}
        BackdropSource::Grid => {
            if let Some(grid) = doc.as_ref().and_then(|doc| doc.document.world().grid()) {
                rewrite(grid, &touched, &chunks, &mut resident);
            }
        }
        BackdropSource::Combat => {
            if let Some(map) = combat.as_ref().and_then(|combat| combat.on_screen()) {
                rewrite(map.content().grid(), &touched, &chunks, &mut resident);
            }
        }
    }
}

fn rewrite<T: TileVocabulary>(
    grid: &TileGrid<T>,
    touched: &[ChunkCoord],
    chunks: &MapChunks,
    resident: &mut Query<(&ChunkCoord, &mut TilemapChunkTileData), With<ChunkCache>>,
) {
    for coord in touched {
        let Some(entity) = chunks.live.get(coord) else {
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
/// An undo or a redo names no cells, so it sets `everything` instead, which rewrites every
/// resident grid chunk. Cleared by [`clear_painted_cells`] once [`repaint_grid_chunks`] has
/// acted, so a stroke redraws once rather than every frame until the next one.
#[derive(Resource, Debug, Default)]
pub struct PaintedCells {
    pub cells: Vec<(u32, u32)>,
    pub everything: bool,
}

/// Whether a stroke, an undo or a redo is waiting to be drawn.
pub fn cells_were_painted(changes: Res<PaintedCells>) -> bool {
    !changes.cells.is_empty() || changes.everything
}

/// Forgets the stroke once it has been drawn.
pub fn clear_painted_cells(mut changes: ResMut<PaintedCells>) {
    changes.cells.clear();
    changes.everything = false;
}

fn to_grid_data<T: TileVocabulary>(tiles: &[Option<T>]) -> Vec<Option<TileData>> {
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
