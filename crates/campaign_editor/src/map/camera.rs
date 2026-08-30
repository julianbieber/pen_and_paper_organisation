//! Where the camera is looking and how far out, within what the terrain allows.

use bevy::camera::{Projection, ScalingMode};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

use crate::map::load::{MapAssets, MapTerrain};
use crate::map::view::MapView;

/// Cells of slack kept around the terrain, so the map is not pinned flush to the edge
/// of the view.
pub const CAMERA_MARGIN_CELLS: f32 = 64.0;

/// The most chunks allowed to be resident at once.
///
/// This, and not the terrain's size, is what bounds how far the map zooms out. Fitting
/// a terrain several thousand cells on a side into the view would make every one of
/// its chunks resident and draw its tiles smaller than a pixel — expensive, and it
/// shimmers.
pub const MAX_RESIDENT_CHUNKS: f32 = 256.0;

const ZOOM_STEP: f32 = 1.2;

/// The one camera the map is seen through.
#[derive(Component, Default, Clone)]
pub struct MapCamera;

/// Spawns the one 2D camera the window draws through.
///
/// `Camera2d` already brings an orthographic projection with it, so only the marker is
/// added here; where it looks and how far out belong to [`frame_terrain`] and
/// [`drive_camera`].
pub fn spawn_camera(mut commands: Commands) {
    commands.spawn((Camera2d, MapCamera));
}

/// Centres the camera on the terrain and fits it in view, once.
///
/// The world origin is the terrain's top-left corner, so without this a large terrain
/// opens looking at a corner of itself.
///
/// Waits for the window to have a real viewport before it settles. A camera's own
/// projection is recomputed in `PostUpdate`, and the window is resized a frame or two
/// after it is created, so framing against whatever the first frame reports produces a
/// zoom fitted to a window that no longer exists.
pub fn frame_terrain(
    terrain: Res<MapTerrain>,
    assets: Res<MapAssets>,
    mut framed: Local<bool>,
    camera: Single<(&mut Transform, &mut Projection, &Camera), With<MapCamera>>,
) {
    if *framed {
        return;
    }
    let (mut transform, mut projection, camera) = camera.into_inner();
    let view = MapView::new(terrain.width, terrain.height, assets.tile_size as f32);
    let extent = view.extent();

    transform.translation.x = extent.center().x;
    transform.translation.y = extent.center().y;

    let Some(viewport) = viewport_of(camera) else {
        return;
    };
    if let Projection::Orthographic(orthographic) = &mut *projection {
        let fit = (extent.width() / viewport.x).max(extent.height() / viewport.y);
        let (low, high) = zoom_bounds(view, viewport);
        orthographic.scaling_mode = ScalingMode::WindowSize;
        orthographic.scale = fit.clamp(low, high);
        *framed = true;
    }
}

/// Pans the map by drag, zooms it about the cursor by wheel, and clamps both to what
/// the terrain allows.
pub fn drive_camera(
    terrain: Res<MapTerrain>,
    assets: Res<MapAssets>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window>,
    camera: Single<(&mut Transform, &mut Projection, &Camera), With<MapCamera>>,
) {
    let (mut transform, mut projection, camera) = camera.into_inner();
    let Projection::Orthographic(orthographic) = &mut *projection else {
        return;
    };
    let Some(viewport) = viewport_of(camera) else {
        return;
    };
    let view = MapView::new(terrain.width, terrain.height, assets.tile_size as f32);
    let per_pixel = orthographic.scale * window.scale_factor();

    if buttons.pressed(MouseButton::Left) && motion.delta != Vec2::ZERO {
        transform.translation.x -= motion.delta.x * per_pixel;
        transform.translation.y += motion.delta.y * per_pixel;
    }

    if scroll.delta.y != 0.0 {
        let (low, high) = zoom_bounds(view, viewport);
        let before = orthographic.scale;
        let after = (before * ZOOM_STEP.powf(-scroll.delta.y)).clamp(low, high);
        if after != before {
            let anchor = window
                .cursor_position()
                .map(|cursor| cursor * window.scale_factor() - viewport / 2.0)
                .map(|offset| Vec2::new(offset.x, -offset.y))
                .unwrap_or(Vec2::ZERO);
            let shift = anchor * (before - after);
            transform.translation.x += shift.x;
            transform.translation.y += shift.y;
            orthographic.scale = after;
        }
    }

    let half = viewport * orthographic.scale / 2.0;
    let bounds = view.extent_with_margin(CAMERA_MARGIN_CELLS);
    transform.translation.x = hold(transform.translation.x, bounds.min.x, bounds.max.x, half.x);
    transform.translation.y = hold(transform.translation.y, bounds.min.y, bounds.max.y, half.y);
}

/// The viewport the projection is actually scaled against.
///
/// Physical, not logical: `ScalingMode::WindowSize` is fed the physical size, so
/// measuring the view in logical pixels puts every zoom bound and every visible-chunk
/// rectangle out by the display's scale factor. `None` until the window has a real
/// size, which it does not on the frame it is created.
pub fn viewport_of(camera: &Camera) -> Option<Vec2> {
    camera
        .physical_viewport_size()
        .map(|size| size.as_vec2())
        .filter(|size| size.min_element() > 1.0)
}

fn hold(centre: f32, low: f32, high: f32, half: f32) -> f32 {
    if high - low <= half * 2.0 {
        return (low + high) / 2.0;
    }
    centre.clamp(low + half, high - half)
}

fn zoom_bounds(view: MapView, viewport: Vec2) -> (f32, f32) {
    let viewport = viewport.max(Vec2::ONE);
    let chunk = view.chunk_size();

    let closest = chunk / viewport.x;
    let budget = chunk * (MAX_RESIDENT_CHUNKS / (viewport.x * viewport.y)).sqrt();
    let extent = view.extent_with_margin(CAMERA_MARGIN_CELLS);
    let whole = (extent.width() / viewport.x).max(extent.height() / viewport.y);

    let furthest = budget.min(whole);
    (closest.min(furthest), closest.max(furthest))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A terrain smaller than one chunk puts the "fits the whole terrain" bound tighter
    // than the "shows one chunk" bound, and `f32::clamp` panics on an inverted range —
    // the fixtures in this repo are 8x8 and 64x64, so this is the common case, not an
    // exotic one.
    #[test]
    fn zoom_bounds_come_back_the_right_way_round_on_a_tiny_terrain() {
        let viewport = Vec2::new(1920.0, 1080.0);
        for (width, height) in [(8, 8), (64, 64), (1, 1), (4096, 4096)] {
            let view = MapView::new(width, height, 16.0);
            let (low, high) = zoom_bounds(view, viewport);
            assert!(low <= high, "{width}x{height} gave {low}..{high}");
            assert!(low.is_finite() && high.is_finite() && low > 0.0);
            let _ = 1.0f32.clamp(low, high);
        }
    }

    // A zero extent must not divide anything into a NaN that then reaches a clamp.
    #[test]
    fn a_terrain_with_no_extent_still_yields_usable_bounds() {
        let (low, high) = zoom_bounds(MapView::new(0, 0, 16.0), Vec2::new(800.0, 600.0));
        assert!(low <= high && low.is_finite() && high.is_finite());
    }

    // When the view is larger than the terrain plus its margin there is no clamp that
    // satisfies it, so the map is centred instead of jumping to an edge.
    #[test]
    fn a_view_wider_than_the_terrain_is_centred_rather_than_clamped() {
        assert_eq!(hold(500.0, -100.0, 100.0, 400.0), 0.0);
        assert_eq!(hold(0.0, 0.0, 1000.0, 100.0), 100.0);
        assert_eq!(hold(500.0, 0.0, 1000.0, 100.0), 500.0);
    }
}
