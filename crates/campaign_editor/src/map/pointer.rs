//! Where the cursor is, in the one coordinate system a world document uses.
//!
//! It sits under `map/` rather than beside the authoring systems because every input it
//! has is the map's — the terrain's size, the tileset's tile size, the camera and the
//! window — and it goes through [`MapView`], the one conversion between cells and world
//! units. Authoring asks it a question; it knows nothing about authoring.

use bevy::camera::Projection;
use bevy::prelude::*;
use campaign::feature::CellPoint;

use crate::map::camera::{MapCamera, viewport_of};
use crate::map::load::{MapAssets, MapTerrain};
use crate::map::view::MapView;

/// How far from a vertex or an edge a press still counts as landing on it, in screen
/// pixels.
///
/// Small: a vertex handle is a deliberate target, and slack wide enough to catch a
/// neighbouring one turns reshaping into guesswork.
pub const PICK_SLACK_PIXELS: f32 = 6.0;

/// How close a vertex being placed has to come before it snaps, in screen pixels.
///
/// Wider than the pick slack, because snapping is a convenience offered while drawing
/// rather than a target being aimed at, and a road endpoint that refuses to join is worse
/// than one that joins when it was nearly meant to.
pub const SNAP_SLACK_PIXELS: f32 = 10.0;

/// Where the cursor is on the map, and what a screen pixel is worth there.
///
/// `cell` is `None` whenever the cursor is outside the window or there is no view to
/// measure against; every authoring system does nothing at all in that case, so nothing
/// ever places a vertex at a position that was never pointed at.
///
/// `cells_per_pixel` is the one factor, not a slack per caller: every slack in the editor
/// is a fixed size on screen, so each caller scales its own through [`MapPointer::slack`]
/// and none of them is a constant in cells.
#[derive(Resource, Debug, Clone, Copy)]
pub struct MapPointer {
    pub cell: Option<CellPoint>,
    pub cells_per_pixel: f32,
}

impl Default for MapPointer {
    fn default() -> Self {
        Self {
            cell: None,
            cells_per_pixel: 1.0,
        }
    }
}

impl MapPointer {
    /// `pixels` of screen slack, in terrain cells at the current zoom.
    pub fn slack(&self, pixels: f32) -> f32 {
        pixels * self.cells_per_pixel
    }
}

/// Turns the cursor into a terrain cell, and states what a screen pixel is worth in cells.
///
/// Reads the camera's own [`Transform`] and its projection scale rather than
/// [`Camera::viewport_to_world_2d`], which goes through `GlobalTransform` and the
/// camera's computed projection — both written in `PostUpdate`, and so both a frame
/// behind a camera this frame's `drive_camera` has just moved. Running after
/// `drive_camera` would buy nothing at all if the position it read were last frame's.
pub fn track_pointer(
    terrain: Res<MapTerrain>,
    assets: Res<MapAssets>,
    window: Single<&Window>,
    camera: Single<(&Transform, &Projection, &Camera), With<MapCamera>>,
    mut pointer: ResMut<MapPointer>,
) {
    let (transform, projection, camera) = camera.into_inner();
    let Projection::Orthographic(orthographic) = projection else {
        return;
    };
    let Some(viewport) = viewport_of(camera) else {
        return;
    };
    let view = MapView::new(terrain.width, terrain.height, assets.tile_size as f32);

    let cells_per_pixel = orthographic.scale * window.scale_factor() / view.cell_size;
    let cell = cursor_offset(&window, viewport).map(|offset| {
        let world = transform.translation.truncate() + offset * orthographic.scale;
        let cell = view.world_to_cell(world);
        CellPoint::new(cell.x, cell.y)
    });

    if pointer.cell != cell || pointer.cells_per_pixel != cells_per_pixel {
        pointer.cell = cell;
        pointer.cells_per_pixel = cells_per_pixel;
    }
}

/// Where the cursor sits relative to the middle of the view, in physical pixels with y
/// running up.
///
/// Multiplying this by the projection's scale gives an offset in world units, which is
/// what both the cursor-to-cell conversion and the camera's zoom anchor need. `None` when
/// the cursor is outside the window.
pub fn cursor_offset(window: &Window, viewport: Vec2) -> Option<Vec2> {
    window
        .cursor_position()
        .map(|cursor| cursor * window.scale_factor() - viewport / 2.0)
        .map(|offset| Vec2::new(offset.x, -offset.y))
}
