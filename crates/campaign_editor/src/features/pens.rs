//! The fixed set of gizmo configuration groups the map is stroked through, and the order
//! they paint in.
//!
//! A gizmo's width and dash come from a configuration group, which is a *type* — so a pen
//! cannot be built at runtime from a [`Stroke`], and the set of them is fixed at compile
//! time. Both the group per stroke and the paint order are therefore written out by hand
//! here, and a stroke added to [`campaign::style`] fails to compile in this file until it
//! is given a pen.
//!
//! Order matters and z does not. Bevy queues every 2D gizmo at the same depth and draws it
//! with the depth comparison always passing, so what covers what is the order the groups
//! are registered in — not the z a line is given, which orders it against the terrain
//! alone.
//!
//! Sizing the pens is a system of its own rather than part of the draw pass because
//! [`Gizmos`] holds the configuration store immutably: nothing that draws can also resize
//! a pen, so the resize runs first and the pass that follows reads what it wrote.

use bevy::camera::Projection;
use bevy::ecs::system::SystemParam;
use bevy::gizmos::config::{GizmoConfigStore, GizmoLineStyle};
use bevy::gizmos::gizmos::GizmoBuffer;
use bevy::prelude::*;
use campaign::style::{Dash, Stroke};

use crate::map::camera::MapCamera;
use crate::map::load::MapAssets;

/// The width of the pen a label is drawn through, in logical pixels.
///
/// Its own pen, and always solid: `Gizmos::text` fans a glyph out into the caller's own
/// configuration group, so a label drawn through a feature's pen would inherit that pen's
/// dash and come out in pieces.
pub const LABEL_WIDTH_PIXELS: f32 = 1.2;

/// How tall a label's capitals are drawn, in logical pixels.
pub const LABEL_PIXELS: f32 = 11.0;

/// The width the draft, the handles and the box-select rectangle are drawn at.
pub const OVERLAY_WIDTH_PIXELS: f32 = 1.5;

/// The widest a river is drawn, in logical pixels.
///
/// A river matches the terrain's own channel, which is one cell wide, so its pen is
/// rewritten from the zoom every frame. The cap is not a look — it is a guard: at the
/// closest zoom a cell covers a good fraction of the window, and a line that wide turns
/// every joint into geometry nobody asked for.
pub const RIVER_CAP_PIXELS: f32 = 48.0;

macro_rules! pen_groups {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Default, Reflect, GizmoConfigGroup)]
            #[reflect(Default)]
            pub struct $name;
        )*
    };
}

pen_groups!(
    GridPen,
    RegionPen,
    BorderPen,
    IconPen,
    TrailPen,
    RoadPen,
    RiverPen,
    LabelPen,
    DraftPen,
    HandlePen,
);

/// Every pen the map draws through, as one system parameter.
///
/// One parameter rather than ten, because a system takes at most sixteen and the draw
/// pass already asks for ten resources of its own.
#[derive(SystemParam)]
pub struct Pens<'w, 's> {
    pub grid: Gizmos<'w, 's, GridPen>,
    pub region: Gizmos<'w, 's, RegionPen>,
    pub border: Gizmos<'w, 's, BorderPen>,
    pub icon: Gizmos<'w, 's, IconPen>,
    pub trail: Gizmos<'w, 's, TrailPen>,
    pub road: Gizmos<'w, 's, RoadPen>,
    pub river: Gizmos<'w, 's, RiverPen>,
    pub label: Gizmos<'w, 's, LabelPen>,
    pub draft: Gizmos<'w, 's, DraftPen>,
    pub handle: Gizmos<'w, 's, HandlePen>,
}

impl Pens<'_, '_> {
    /// The pen `stroke` names.
    ///
    /// Exhaustive with no catch-all arm on purpose: a stroke added later must be given a
    /// group here as well as a place in the paint order, and neither can be forgotten
    /// silently.
    pub fn stroke(&mut self, stroke: Stroke) -> &mut dyn StrokePen {
        match stroke {
            Stroke::Grid => &mut self.grid,
            Stroke::Region => &mut self.region,
            Stroke::Border => &mut self.border,
            Stroke::Icon => &mut self.icon,
            Stroke::Trail => &mut self.trail,
            Stroke::Road => &mut self.road,
            Stroke::River => &mut self.river,
        }
    }
}

/// Drawing a line through a pen without naming which pen it is.
///
/// Exists only so that [`Pens::stroke`] can hand one back: the `Gizmos` fields all have
/// different types, and every one of them draws a line the same way.
pub trait StrokePen {
    fn line(&mut self, from: Vec2, to: Vec2, depth: f32, colour: Color);
}

impl<T: bevy::gizmos::config::GizmoConfigGroup> StrokePen for Gizmos<'_, '_, T> {
    fn line(&mut self, from: Vec2, to: Vec2, depth: f32, colour: Color) {
        GizmoBuffer::line(self, from.extend(depth), to.extend(depth), colour);
    }
}

/// Registers every pen, in the order they paint.
///
/// The registration order is the paint order — the grid, then the region fills, then the
/// feature strokes, then labels, then the draft, then the handles — so the handles a GM is
/// dragging are never hidden under the shape they belong to, and a dungeon's grid rules the
/// backdrop without ruling over what is drawn on it.
pub fn register_pens(app: &mut App) {
    app.init_gizmo_group::<GridPen>()
        .init_gizmo_group::<RegionPen>()
        .init_gizmo_group::<BorderPen>()
        .init_gizmo_group::<IconPen>()
        .init_gizmo_group::<TrailPen>()
        .init_gizmo_group::<RoadPen>()
        .init_gizmo_group::<RiverPen>()
        .init_gizmo_group::<LabelPen>()
        .init_gizmo_group::<DraftPen>()
        .init_gizmo_group::<HandlePen>();
}

/// Writes every pen's width and dash into the configuration store.
///
/// Runs before anything draws, because a pen cannot be resized by a system that is also
/// holding one.
///
/// This is the one place in the editor where a logical size becomes a physical one: the
/// line shader measures its width against the framebuffer, so a width is multiplied by the
/// window's scale factor here and nowhere else. Every threshold, slack and reveal scale
/// stays logical.
///
/// The river is the exception that makes the system run every frame rather than only when
/// the display changes: its width is one terrain cell, so it is rewritten from the zoom.
/// It is floored at one physical pixel because below that the line shader fades the line's
/// own alpha, which would compound with the detail fade and make a river vanish twice as
/// fast as it should.
pub fn size_pens(
    window: Single<&Window>,
    assets: Res<MapAssets>,
    camera: Single<&Projection, With<MapCamera>>,
    mut store: ResMut<GizmoConfigStore>,
) {
    let factor = window.scale_factor();
    let Projection::Orthographic(orthographic) = *camera else {
        return;
    };
    for stroke in Stroke::all() {
        let logical = match stroke {
            Stroke::River => river_pixels(assets.tile_size as f32, orthographic.scale),
            other => other.width_pixels(),
        };
        let width = (logical * factor).max(1.0);
        let style = line_style(stroke.dash());
        match stroke {
            Stroke::Grid => set::<GridPen>(&mut store, width, style),
            Stroke::Region => set::<RegionPen>(&mut store, width, style),
            Stroke::Border => set::<BorderPen>(&mut store, width, style),
            Stroke::Icon => set::<IconPen>(&mut store, width, style),
            Stroke::Trail => set::<TrailPen>(&mut store, width, style),
            Stroke::Road => set::<RoadPen>(&mut store, width, style),
            Stroke::River => set::<RiverPen>(&mut store, width, style),
        }
    }

    let solid = line_style(Dash::Solid);
    set::<LabelPen>(&mut store, (LABEL_WIDTH_PIXELS * factor).max(1.0), solid);
    set::<DraftPen>(&mut store, (OVERLAY_WIDTH_PIXELS * factor).max(1.0), solid);
    set::<HandlePen>(&mut store, (OVERLAY_WIDTH_PIXELS * factor).max(1.0), solid);
}

/// How wide a river is drawn at this zoom, in logical pixels: one terrain cell, capped.
///
/// `cell_size` is world units to the cell and `scale` is world units to the logical pixel,
/// so their ratio is the cell's width on screen.
pub fn river_pixels(cell_size: f32, scale: f32) -> f32 {
    if !scale.is_finite() || scale <= 0.0 {
        return Stroke::River.width_pixels();
    }
    (cell_size / scale).clamp(1.0, RIVER_CAP_PIXELS)
}

fn line_style(dash: Dash) -> GizmoLineStyle {
    match dash {
        Dash::Solid => GizmoLineStyle::Solid,
        Dash::Dotted => GizmoLineStyle::Dotted,
        Dash::Dashed { line, gap } => GizmoLineStyle::Dashed {
            line_scale: line,
            gap_scale: gap,
        },
    }
}

fn set<T: bevy::gizmos::config::GizmoConfigGroup>(
    store: &mut GizmoConfigStore,
    width: f32,
    style: GizmoLineStyle,
) {
    let (config, _) = store.config_mut::<T>();
    config.line.width = width;
    config.line.style = style;
}

#[cfg(test)]
mod tests {
    use super::*;

    // The river matches the terrain's own one-cell channel, so its width has to track the
    // zoom rather than sit at a constant. The floor is what keeps the line shader from
    // fading the river's alpha on its own, which would compound with the detail fade.
    #[test]
    fn a_river_is_one_cell_wide_between_its_floor_and_its_cap() {
        assert_eq!(river_pixels(16.0, 1.0), 16.0);
        assert_eq!(river_pixels(16.0, 0.5), 32.0);
        assert_eq!(river_pixels(16.0, 64.0), 1.0, "a hair-thin river is still one pixel");
        assert_eq!(river_pixels(16.0, 0.01), RIVER_CAP_PIXELS, "capped, not unbounded");
    }

    // A projection scale of zero or a NaN reaches here from a camera that has not been
    // framed yet, and a width of NaN silently drops every river the frame draws.
    #[test]
    fn an_unusable_zoom_falls_back_to_the_tables_own_width() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(river_pixels(16.0, scale), Stroke::River.width_pixels());
        }
    }
}
