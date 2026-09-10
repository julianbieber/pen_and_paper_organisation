//! The bar in the corner saying how far a stretch of screen is.
//!
//! Which figure it is labelled with, and which unit that figure reads best in, are
//! [`campaign::measure`]'s answers. What is left here is a node's width and a string.

use bevy::camera::Projection;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::scene::CommandsSceneExt;
use campaign::measure;

use crate::document::WorldDoc;
use crate::map::backdrop::Backdrop;
use crate::map::camera::{MapCamera, viewport_of};
use crate::OpenCampaign;

/// The most of the viewport's width the bar may span.
///
/// The bar is the largest round figure that fits inside this, so the fraction is also what
/// decides how often the figure steps.
pub const BAR_VIEWPORT_FRACTION: f32 = 0.25;

/// The bar itself, whose width is the distance its label names.
#[derive(Component, Default, Clone)]
pub struct ScaleBarTrack;

/// The line naming what the bar spans.
#[derive(Component, Default, Clone)]
pub struct ScaleBarLabel;

/// The bar's root, so it can be taken off screen where there is no scale to show.
#[derive(Component, Default, Clone)]
pub struct ScaleBarRoot;

/// Hangs the scale bar over the map, once there is a map to measure.
pub fn build_scale_bar(mut commands: Commands) {
    commands.spawn_scene(bar());
}

/// Sizes the bar to a round figure for the current zoom and labels it.
///
/// Reads the projection directly rather than any world-to-screen helper, because those are
/// written in `PostUpdate` and would be a frame behind the zoom this is describing.
///
/// Writes the width and the label only where they differ: a `Text` write re-shapes glyphs,
/// which is dearer than the layout an unconditional `Node` write already costs.
pub fn show_scale_bar(
    backdrop: Res<Backdrop>,
    doc: Option<Res<WorldDoc>>,
    open: Res<OpenCampaign>,
    camera: Single<(&Projection, &Camera), With<MapCamera>>,
    mut roots: Query<&mut Node, (With<ScaleBarRoot>, Without<ScaleBarTrack>)>,
    mut tracks: Query<&mut Node, With<ScaleBarTrack>>,
    mut labels: Query<&mut Text, With<ScaleBarLabel>>,
) {
    let (projection, camera) = camera.into_inner();
    let bar = scale_bar_of(projection, camera, &backdrop, doc.as_deref(), &open);

    let display = match bar {
        Some(_) => Display::Flex,
        None => Display::None,
    };
    for mut node in roots.iter_mut() {
        if node.display != display {
            node.display = display;
        }
    }

    let Some((figure, unit, pixels)) = bar else {
        return;
    };
    let width = px(pixels);
    for mut node in tracks.iter_mut() {
        if node.width != width {
            node.width = width;
        }
    }
    let shown = measure::figure(figure, &unit);
    for mut text in labels.iter_mut() {
        if text.0 != shown {
            text.0 = shown.clone();
        }
    }
}

fn scale_bar_of(
    projection: &Projection,
    camera: &Camera,
    backdrop: &Backdrop,
    doc: Option<&WorldDoc>,
    open: &OpenCampaign,
) -> Option<(f64, String, f32)> {
    let Projection::Orthographic(orthographic) = projection else {
        return None;
    };
    let viewport = viewport_of(camera)?;
    let doc = doc?;

    let worth = measure::worth_of(doc.document.world(), open.0.manifest());
    let cells_per_pixel = orthographic.scale / backdrop.view.cell_size;
    let units_per_pixel = worth.units(cells_per_pixel);

    measure::scale_bar(
        units_per_pixel,
        &worth,
        viewport.x * BAR_VIEWPORT_FRACTION,
    )
}

fn bar() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            bottom: px(96),
            left: px(12),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexStart,
            row_gap: px(4),
            padding: px(8),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        ScaleBarRoot
        Children [
            (Text("") ThemedText ScaleBarLabel),
            (
                Node {
                    width: px(0),
                    height: px(6),
                }
                BackgroundColor(Color::WHITE)
                ScaleBarTrack
            )
        ]
    }
}
