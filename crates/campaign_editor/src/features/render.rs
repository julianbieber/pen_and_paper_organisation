//! Drawing the features in view, the draft, the selection and its handles.
//!
//! Every colour, width, dash, hatch and icon comes from [`campaign::style`]; every
//! decision about whether a feature is drawn at all comes from [`campaign::lod`]; every
//! label position comes from [`campaign::label`]. What is left here is ECS and pixels,
//! which is the whole reason those three live in a crate that can be tested without a
//! window.
//!
//! Immediate mode, through gizmos rather than entities: the document is the only source of
//! truth for what is on the map, and an entity per feature would be a second one that has
//! to be kept in step with every edit, every undo and every redo.
//!
//! Every stage is bounded by the view. A feature is culled against the visible rectangle
//! before it is styled, a hatch is clipped to that rectangle and capped per polygon and
//! per frame, and at most [`MAX_LABELS`] labels are placed — so the pass costs what is on
//! screen rather than what the document holds.
//!
//! Two facts about bevy's gizmos shape everything here. A 2D gizmo carries no depth
//! against another gizmo, so what covers what is the order the pens are registered in and
//! a z coordinate orders a line against the terrain alone. And the line shader measures a
//! dash along one segment, restarting it at every vertex, so a dashed pen has to be fed
//! segments at least one dash period long or it draws solid.

use bevy::camera::Projection;
use bevy::color::palettes::css;
use bevy::prelude::*;
use campaign::feature::{CellPoint, Feature, Geometry};
use campaign::label::{self, LabelBox, LabelCandidate, MAX_LABELS};
use campaign::lod;
use campaign::style::{self, Fill, IconShape, Pattern, Stroke, Style};
use campaign::{FeatureId, gesture};

use crate::document::WorldDoc;
use crate::features::draw::Drafting;
use crate::features::pens::{LABEL_PIXELS, Pens};
use crate::features::select::{DragKind, Dragging, Selection};
use crate::features::tool::ActiveTool;
use crate::map::camera::{MapCamera, viewport_of};
use crate::map::view::{FEATURE_Z, MapView};
use campaign::tiles::{GRID_COARSE_CELLS, grid_lines};

const HANDLE_PIXELS: f32 = 4.0;
const MAX_FILL_LINES: usize = 512;
const MAX_FILL_LINES_PER_FRAME: usize = 8_000;

const DRAFT_Z: f32 = FEATURE_Z + 0.1;
const HANDLE_Z: f32 = FEATURE_Z + 0.2;

/// Draws the features the camera can see, the draft, the selection and its handles.
///
/// This is the one per-frame pass over the whole document.
///
/// A drag in flight is drawn through the same function that will build its edit on
/// release, so what is shown and what is committed cannot disagree; a draft is drawn with
/// a segment running to the pointer, and an available snap is marked, so what the next
/// click would commit is visible before it is committed.
///
/// Sizes are measured in world units to the logical pixel and thresholds in terrain cells
/// to the logical pixel; nothing here converts to physical pixels, which only a pen's
/// width does.
///
/// A hatch line sits at a whole multiple of its spacing from the world origin, so panning
/// changes which lines of a region are drawn and never where they are.
pub fn render_features(
    doc: Res<WorldDoc>,
    selection: Res<Selection>,
    drafting: Res<Drafting>,
    dragging: Res<Dragging>,
    active: Res<ActiveTool>,
    pointer: Res<crate::map::pointer::MapPointer>,
    backdrop: Res<crate::map::backdrop::Backdrop>,
    camera: Single<(&Transform, &Projection, &Camera), With<MapCamera>>,
    mut pens: Pens,
    mut points: Local<Vec<Vec2>>,
) {
    let (transform, projection, camera) = camera.into_inner();
    let Projection::Orthographic(orthographic) = projection else {
        return;
    };
    let Some(viewport) = viewport_of(camera) else {
        return;
    };
    let view = backdrop.view;

    let half = viewport * orthographic.scale / 2.0;
    let centre = transform.translation.truncate();
    let visible = Rect::from_corners(centre - half, centre + half);

    if let Some(grid) = doc.document.world().grid() {
        draw_grid(&mut pens, view, grid, visible, pointer.cells_per_pixel);
    }

    let per_pixel = orthographic.scale;
    let cells_per_pixel = pointer.cells_per_pixel;
    let handle = HANDLE_PIXELS * per_pixel;

    let world = doc.document.world();
    let dragged: Vec<(FeatureId, Vec<CellPoint>)> = if dragging.what == Some(DragKind::Body) {
        gesture::dragged(world, &selection.features, dragging.offset())
    } else {
        Vec::new()
    };

    let mut candidates: Vec<LabelCandidate> = Vec::new();
    let mut boxes: Vec<LabelBox> = Vec::new();
    let mut fill_budget = MAX_FILL_LINES_PER_FRAME;

    for (id, feature) in world.features() {
        let held = selection.features.contains(&id);
        let vertices: &[CellPoint] = dragged
            .iter()
            .find(|(dragged_id, _)| *dragged_id == id)
            .map(|(_, moved)| moved.as_slice())
            .unwrap_or_else(|| feature.geometry.vertices());

        points.clear();
        points.extend(
            vertices
                .iter()
                .map(|vertex| view.cell_to_world(vertex.x, vertex.y)),
        );
        if !touches(&points, visible, handle) {
            continue;
        }

        let detail = lod::detail(world, &doc.areas, cells_per_pixel, id, &selection.features);
        if detail <= 0.0 {
            continue;
        }

        let style = style::of(feature.kind, feature.rank);
        let colour = if held {
            Color::from(css::GOLD).with_alpha(detail)
        } else {
            Color::srgba(style.red, style.green, style.blue, style.alpha * detail)
        };
        let closed = matches!(feature.geometry, Geometry::Polygon(_));

        if closed
            && let Some(fill) = style.fill
        {
            let drawn = hatch(
                &mut pens,
                &points,
                visible,
                fill,
                per_pixel,
                colour,
                fill_budget.min(MAX_FILL_LINES),
            );
            fill_budget = fill_budget.saturating_sub(drawn);
        }

        stroke(&mut pens, &style, &points, closed, colour, per_pixel);

        if style.icon.is_some()
            && matches!(feature.geometry, Geometry::Point(_))
            && let Some(at) = points.first()
        {
            let radius = style::icon_pixels(feature.rank) * per_pixel / 2.0;
            draw_icon(&mut pens, &style, *at, radius, colour);
        }

        if !feature.label.is_empty() {
            offer_label(
                &mut candidates,
                &mut boxes,
                world,
                id,
                feature,
                &style,
                detail,
                cells_per_pixel,
            );
        }

        if held {
            for (index, point) in points.iter().enumerate() {
                let selected = selection
                    .vertex
                    .is_some_and(|vertex| vertex.feature == id && vertex.index == index);
                let colour_of = if selected { css::ORANGE_RED } else { css::GOLD };
                mark(&mut pens.handle, *point, handle, Color::from(colour_of), HANDLE_Z);
            }
        }
    }

    for placed in label::place(&candidates, &boxes, cells_per_pixel, MAX_LABELS) {
        let Some(feature) = world.feature(placed.feature) else {
            continue;
        };
        let at = view.cell_to_world(placed.anchor.x, placed.anchor.y);
        let style = style::of(feature.kind, feature.rank);
        let colour = Color::srgba(style.red, style.green, style.blue, placed.fade);
        pens.label.text(
            Isometry3d::from_translation(at.extend(HANDLE_Z)),
            &feature.label,
            LABEL_PIXELS * per_pixel,
            Vec2::ZERO,
            colour,
        );
    }

    if let Some(draft) = drafting.draft.as_ref() {
        points.clear();
        points.extend(
            draft
                .vertices()
                .iter()
                .map(|vertex| view.cell_to_world(vertex.x, vertex.y)),
        );
        if let Some(cell) = drafting.snap.map(|snap| snap.at).or(pointer.cell) {
            points.push(view.cell_to_world(cell.x, cell.y));
        }
        let colour = Color::from(css::AQUAMARINE);
        for pair in points.windows(2) {
            pens.draft.line(pair[0].extend(DRAFT_Z), pair[1].extend(DRAFT_Z), colour);
        }
        for point in points.iter() {
            mark(&mut pens.draft, *point, handle, colour, DRAFT_Z);
        }
    }

    if active.drawing()
        && let Some(snap) = drafting.snap.filter(|snap| snap.landed())
    {
        let at = view.cell_to_world(snap.at.x, snap.at.y);
        mark(&mut pens.draft, at, handle * 2.0, Color::from(css::SPRING_GREEN), DRAFT_Z);
    }

    if dragging.what == Some(DragKind::Box) {
        let from = view.cell_to_world(dragging.grabbed.x, dragging.grabbed.y);
        let to = view.cell_to_world(dragging.latest.x, dragging.latest.y);
        let rect = Rect::from_corners(from, to);
        pens.handle
            .rect_2d(rect.center(), rect.size(), Color::from(css::AQUAMARINE));
    }
}

fn stroke(
    pens: &mut Pens,
    style: &Style,
    points: &[Vec2],
    closed: bool,
    colour: Color,
    per_pixel: f32,
) {
    let period = style.stroke.dash().period() * style.stroke.width_pixels() * per_pixel;
    let pen = pens.stroke(style.stroke);

    for (from, to) in segments_of(points, closed, period) {
        pen.line(from, to, FEATURE_Z, colour);
    }
}

fn segments_of(points: &[Vec2], closed: bool, period: f32) -> Vec<(Vec2, Vec2)> {
    if points.len() < 2 {
        return Vec::new();
    }
    let mut segments = Vec::with_capacity(points.len());
    let mut from = points[0];
    let last = points.len() - 1;
    for (index, to) in points.iter().enumerate().skip(1) {
        let final_vertex = index == last && !closed;
        if !final_vertex && from.distance(*to) < period {
            continue;
        }
        segments.push((from, *to));
        from = *to;
    }
    if closed && points.len() > 2 {
        segments.push((from, points[0]));
    }
    segments
}

fn hatch(
    pens: &mut Pens,
    points: &[Vec2],
    visible: Rect,
    fill: Fill,
    per_pixel: f32,
    colour: Color,
    budget: usize,
) -> usize {
    if points.len() < 3 || budget == 0 {
        return 0;
    }
    let spacing = fill.spacing_pixels * per_pixel;
    if !spacing.is_finite() || spacing <= 0.0 {
        return 0;
    }

    let mut drawn = 0;
    let angles: &[f32] = match fill.pattern {
        Pattern::Hatch => &[0.0],
        Pattern::CrossHatch => &[0.0, std::f32::consts::FRAC_PI_2],
    };
    for extra in angles {
        drawn += hatch_at(
            pens,
            points,
            visible,
            fill.angle + extra,
            spacing,
            colour,
            budget - drawn.min(budget),
        );
    }
    drawn
}

fn hatch_at(
    pens: &mut Pens,
    points: &[Vec2],
    visible: Rect,
    angle: f32,
    spacing: f32,
    colour: Color,
    budget: usize,
) -> usize {
    let (sin, cos) = angle.sin_cos();
    let into = |point: Vec2| Vec2::new(point.x * cos + point.y * sin, -point.x * sin + point.y * cos);
    let out = |point: Vec2| Vec2::new(point.x * cos - point.y * sin, point.x * sin + point.y * cos);

    let rotated: Vec<Vec2> = points.iter().copied().map(into).collect();
    let mut ring = Rect::from_corners(rotated[0], rotated[0]);
    for point in &rotated {
        ring = ring.union_point(*point);
    }

    let corners = [
        visible.min,
        Vec2::new(visible.max.x, visible.min.y),
        visible.max,
        Vec2::new(visible.min.x, visible.max.y),
    ];
    let mut seen = Rect::from_corners(into(corners[0]), into(corners[0]));
    for corner in corners {
        seen = seen.union_point(into(corner));
    }

    let low = ring.min.y.max(seen.min.y);
    let high = ring.max.y.min(seen.max.y);
    if low > high {
        return 0;
    }

    let first = (low / spacing).ceil() as i64;
    let last = (high / spacing).floor() as i64;
    if last < first {
        return 0;
    }

    let mut drawn = 0;
    let mut crossings: Vec<f32> = Vec::new();
    for k in first..=last {
        if drawn >= budget {
            break;
        }
        let row = k as f32 * spacing;
        crossings.clear();
        for current in 0..rotated.len() {
            let a = rotated[current];
            let b = rotated[(current + 1) % rotated.len()];
            if (a.y > row) == (b.y > row) {
                continue;
            }
            let step = (row - a.y) / (b.y - a.y);
            crossings.push(a.x + (b.x - a.x) * step);
        }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        for span in crossings.as_chunks::<2>().0 {
            if drawn >= budget {
                break;
            }
            let from = span[0].max(seen.min.x);
            let to = span[1].min(seen.max.x);
            if from >= to {
                continue;
            }
            pens.region.line(
                out(Vec2::new(from, row)).extend(FEATURE_Z),
                out(Vec2::new(to, row)).extend(FEATURE_Z),
                colour,
            );
            drawn += 1;
        }
    }
    drawn
}

#[allow(clippy::too_many_arguments)]
fn offer_label(
    candidates: &mut Vec<LabelCandidate>,
    boxes: &mut Vec<LabelBox>,
    world: &campaign::World,
    id: FeatureId,
    feature: &Feature,
    style: &Style,
    detail: f32,
    cells_per_pixel: f32,
) {
    let lift = (style::icon_pixels(feature.rank) / 2.0 + label::LABEL_LIFT_PIXELS) * cells_per_pixel;
    candidates.push(LabelCandidate {
        feature: id,
        anchor: label::anchor(&feature.geometry, lift),
        priority: label::priority(style.label_priority, gesture::depth(world, id)),
        fade: detail,
    });
    boxes.push(LabelBox::of(&feature.label, LABEL_PIXELS));
}

fn draw_icon(pens: &mut Pens, style: &Style, at: Vec2, radius: f32, colour: Color) {
    let pen = pens.stroke(style.stroke);
    match style.icon {
        None => {}
        Some(IconShape::Dot) => polygon(pen, at, radius, 6, 0.0, colour),
        Some(IconShape::Ring) => {
            polygon(pen, at, radius, 8, 0.0, colour);
            polygon(pen, at, radius / 2.0, 6, 0.0, colour);
        }
        Some(IconShape::Star) => {
            polygon(pen, at, radius, 8, 0.0, colour);
            for spoke in 0..4 {
                let angle = spoke as f32 * std::f32::consts::FRAC_PI_4;
                let arm = Vec2::from_angle(angle) * radius;
                pen.line(at - arm, at + arm, FEATURE_Z, colour);
            }
        }
        Some(IconShape::Gate) => {
            let half = Vec2::splat(radius);
            let corners = [
                at + Vec2::new(-half.x, -half.y),
                at + Vec2::new(half.x, -half.y),
                at + Vec2::new(half.x, half.y),
                at + Vec2::new(-half.x, half.y),
            ];
            for index in 0..corners.len() {
                pen.line(corners[index], corners[(index + 1) % 4], FEATURE_Z, colour);
            }
            pen.line(corners[0], corners[2], FEATURE_Z, colour);
        }
    }
}

fn polygon(pen: &mut dyn crate::features::pens::StrokePen, at: Vec2, radius: f32, sides: usize, phase: f32, colour: Color) {
    let step = std::f32::consts::TAU / sides as f32;
    let mut from = at + Vec2::from_angle(phase) * radius;
    for side in 1..=sides {
        let to = at + Vec2::from_angle(phase + step * side as f32) * radius;
        pen.line(from, to, FEATURE_Z, colour);
        from = to;
    }
}

fn mark<T: bevy::gizmos::config::GizmoConfigGroup>(
    gizmos: &mut Gizmos<T>,
    at: Vec2,
    radius: f32,
    colour: Color,
    depth: f32,
) {
    gizmos.circle(Isometry3d::from_translation(at.extend(depth)), radius, colour);
}

fn touches(points: &[Vec2], visible: Rect, slack: f32) -> bool {
    let Some(first) = points.first() else {
        return false;
    };
    let mut bounds = Rect::from_corners(*first, *first);
    for point in points {
        bounds = bounds.union_point(*point);
    }
    let bounds = bounds.inflate(slack);
    bounds.min.x <= visible.max.x
        && bounds.max.x >= visible.min.x
        && bounds.min.y <= visible.max.y
        && bounds.max.y >= visible.min.y
}

fn draw_grid(
    pens: &mut Pens,
    view: MapView,
    grid: &campaign::grid::TileGrid,
    visible: Rect,
    cells_per_pixel: f32,
) {
    let levels = grid_lines(cells_per_pixel);
    if levels.draws_nothing() {
        return;
    }
    let style = campaign::style::GRID_LINE;
    let top_left = view.cell_to_world(0.0, 0.0) - Vec2::splat(view.cell_size / 2.0) * Vec2::new(1.0, -1.0);
    let full = Rect::from_corners(
        top_left,
        top_left + Vec2::new(grid.width() as f32, -(grid.height() as f32)) * view.cell_size,
    );

    for (spacing, strength) in [(1u32, levels.fine), (GRID_COARSE_CELLS, levels.coarse)] {
        if strength <= 0.0 {
            continue;
        }
        let pen = pens.stroke(Stroke::Grid);
        let colour = Color::srgba(
            style[0],
            style[1],
            style[2],
            campaign::style::GRID_LINE_ALPHA * strength,
        );
        let step = spacing as f32 * view.cell_size;

        let first = ((visible.min.x - full.min.x) / step).floor().max(0.0) as u32;
        let last = ((visible.max.x - full.min.x) / step).ceil().max(0.0) as u32;
        for column in (first..=last.min(grid.width())).step_by(spacing as usize) {
            let x = full.min.x + column as f32 * view.cell_size;
            pen.line(
                Vec2::new(x, full.min.y.max(visible.min.y)),
                Vec2::new(x, full.max.y.min(visible.max.y)),
                FEATURE_Z,
                colour,
            );
        }

        let first = ((full.max.y - visible.max.y) / step).floor().max(0.0) as u32;
        let last = ((full.max.y - visible.min.y) / step).ceil().max(0.0) as u32;
        for row in (first..=last.min(grid.height())).step_by(spacing as usize) {
            let y = full.max.y - row as f32 * view.cell_size;
            pen.line(
                Vec2::new(full.min.x.max(visible.min.x), y),
                Vec2::new(full.max.x.min(visible.max.x), y),
                FEATURE_Z,
                colour,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A solid pen must be handed the polyline exactly as it was authored: coalescing there
    // would move a vertex the GM placed.
    #[test]
    fn a_solid_pen_strokes_every_segment_the_feature_holds() {
        let points = vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)];
        assert_eq!(segments_of(&points, false, 0.0).len(), 2);
    }

    // The line shader restarts a dash at every vertex, so a road authored at cell density
    // would be a run of dash-openings and read as solid. Merging short segments is what
    // makes a dashed pen actually dash.
    #[test]
    fn a_dashed_pen_is_never_handed_a_segment_shorter_than_its_period() {
        let points: Vec<Vec2> = (0..100).map(|n| Vec2::new(n as f32, 0.0)).collect();
        let segments = segments_of(&points, false, 12.0);

        assert!(segments.len() < points.len() / 8, "nothing was coalesced");
        for (from, to) in &segments[..segments.len() - 1] {
            assert!(
                from.distance(*to) >= 12.0,
                "a segment of {} was handed to a pen dashing at 12",
                from.distance(*to)
            );
        }
    }

    // Coalescing may never shorten a line: the last vertex is kept whatever its distance
    // from the one before it, or a road would stop short of the town it runs to.
    #[test]
    fn coalescing_keeps_the_first_and_last_vertex() {
        let points = vec![
            Vec2::ZERO,
            Vec2::new(50.0, 0.0),
            Vec2::new(50.5, 0.0),
            Vec2::new(51.0, 0.0),
        ];
        let segments = segments_of(&points, false, 12.0);
        assert_eq!(segments.first().unwrap().0, Vec2::ZERO);
        assert_eq!(segments.last().unwrap().1, Vec2::new(51.0, 0.0));
    }

    // A polygon is closed by the renderer rather than by its geometry, so the closing edge
    // has to survive coalescing — without it a region is drawn as an open shape and its
    // hatch appears to leak.
    #[test]
    fn a_polygon_is_always_closed_back_to_its_first_vertex() {
        let ring = vec![Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(40.0, 40.0)];
        let segments = segments_of(&ring, true, 12.0);
        assert_eq!(segments.last().unwrap().1, Vec2::ZERO);
    }

    // A point feature is one vertex and has no outline at all; stroking it must draw
    // nothing rather than a zero-length line the shader would still rasterize.
    #[test]
    fn a_single_vertex_is_stroked_as_nothing() {
        assert!(segments_of(&[Vec2::ZERO], false, 0.0).is_empty());
        assert!(segments_of(&[], false, 0.0).is_empty());
    }
}
