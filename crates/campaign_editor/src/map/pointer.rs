//! Where the cursor is, in the one coordinate system a map document uses.
//!
//! It sits under `map/` rather than beside the authoring systems because every input it
//! has is the map's — the live backdrop, the camera and the window — and it goes through
//! that backdrop's [`MapView`](crate::map::view::MapView), the one conversion between
//! cells and world units. Authoring asks it a question; it knows nothing about authoring.

use bevy::camera::Projection;
use bevy::prelude::*;
use campaign::feature::CellPoint;

use crate::map::backdrop::Backdrop;
use crate::map::camera::{MapCamera, viewport_of};

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
///
/// **Logical** pixels, because that is what the projection's scale is measured in — and
/// because a reveal threshold written into `world.ron` must mean the same thing on a
/// display whose scale factor is two as on one whose factor is one.
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

/// A cell the pointer is to be treated as being over, whatever the cursor is doing.
///
/// Set by the control socket so a scripted run can put the pointer somewhere without a
/// real mouse. Empty in an ordinary session, and [`track_pointer`] then reads the cursor
/// as it always did — the override is a way to answer the same question, never a second
/// path into the authoring systems.
#[derive(Resource, Debug, Default)]
pub struct PointerOverride(pub Option<CellPoint>);

/// Turns the cursor into a cell of the live backdrop, and states what a screen pixel is
/// worth in cells.
///
/// Reads the camera's own [`Transform`] and its projection scale rather than
/// [`Camera::viewport_to_world_2d`], which goes through `GlobalTransform` and the
/// camera's computed projection — both written in `PostUpdate`, and so both a frame
/// behind a camera this frame's `drive_camera` has just moved. Running after
/// `drive_camera` would buy nothing at all if the position it read were last frame's.
pub fn track_pointer(
    backdrop: Res<Backdrop>,
    window: Single<&Window>,
    camera: Single<(&Transform, &Projection, &Camera), With<MapCamera>>,
    forced: Res<PointerOverride>,
    mut pointer: ResMut<MapPointer>,
) {
    let (transform, projection, camera) = camera.into_inner();
    let Projection::Orthographic(orthographic) = projection else {
        return;
    };
    let Some(viewport) = viewport_of(camera) else {
        return;
    };
    let view = backdrop.view;

    let cells_per_pixel = orthographic.scale / view.cell_size;
    let cell = forced.0.or_else(|| {
        cursor_offset(&window, viewport).map(|offset| {
            let world = transform.translation.truncate() + offset * orthographic.scale;
            let cell = view.world_to_cell(world);
            CellPoint::new(cell.x, cell.y)
        })
    });

    if pointer.cell != cell || pointer.cells_per_pixel != cells_per_pixel {
        pointer.cell = cell;
        pointer.cells_per_pixel = cells_per_pixel;
    }
}

/// Where the cursor sits relative to the middle of the view, in logical pixels with y
/// running up.
///
/// Multiplying this by the projection's scale gives an offset in world units, which is
/// what both the cursor-to-cell conversion and the camera's zoom anchor need — and which
/// is why it is logical: the scale is world units to the logical pixel, so a cursor
/// converted to physical pixels first would land the offset out by the display's scale
/// factor. `None` when the cursor is outside the window.
pub fn cursor_offset(window: &Window, viewport: Vec2) -> Option<Vec2> {
    window
        .cursor_position()
        .map(|cursor| cursor - viewport / 2.0)
        .map(|offset| Vec2::new(offset.x, -offset.y))
}
