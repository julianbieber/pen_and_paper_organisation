//! Drawing the features in view, the draft, the selection and its handles.
//!
//! Plainly and uniformly: one width, one colour, whatever the feature is. Styling by kind
//! and revealing a city's interior as the map zooms in belong to issue #5, which replaces
//! this — so nothing here is worth making per-kind now.
//!
//! Immediate mode, through gizmos rather than entities: the document is the only source of
//! truth for what is on the map, and an entity per feature would be a second one that has
//! to be kept in step with every edit, every undo and every redo.

use bevy::camera::Projection;
use bevy::color::palettes::css;
use bevy::prelude::*;
use campaign::feature::{CellPoint, Geometry};
use campaign::gesture;

use crate::features::doc::WorldDoc;
use crate::features::draw::Drafting;
use crate::features::select::{DragKind, Dragging, Selection};
use crate::features::tool::ActiveTool;
use crate::map::camera::{MapCamera, viewport_of};
use crate::map::load::{MapAssets, MapTerrain};
use crate::map::view::{FEATURE_Z, MapView};

const HANDLE_PIXELS: f32 = 4.0;
const DRAFT_Z: f32 = FEATURE_Z + 0.1;
const HANDLE_Z: f32 = FEATURE_Z + 0.2;

/// Draws the features the camera can see, the draft, the selection and its handles.
///
/// This is the one per-frame pass over the whole document, so it is bounded by what is on
/// screen rather than by how much has been drawn on the map.
///
/// A drag in flight is drawn through the same function that will build its edit on
/// release, so what is shown and what is committed cannot disagree; a draft is drawn with
/// a segment running to the pointer, and an available snap is marked, so what the next
/// click would commit is visible before it is committed.
pub fn render_features(
    doc: Res<WorldDoc>,
    selection: Res<Selection>,
    drafting: Res<Drafting>,
    dragging: Res<Dragging>,
    active: Res<ActiveTool>,
    pointer: Res<crate::map::pointer::MapPointer>,
    terrain: Res<MapTerrain>,
    assets: Res<MapAssets>,
    camera: Single<(&Transform, &Projection, &Camera), With<MapCamera>>,
    mut gizmos: Gizmos,
) {
    let (transform, projection, camera) = camera.into_inner();
    let Projection::Orthographic(orthographic) = projection else {
        return;
    };
    let Some(viewport) = viewport_of(camera) else {
        return;
    };
    let view = MapView::new(terrain.width, terrain.height, assets.tile_size as f32);

    let half = viewport * orthographic.scale / 2.0;
    let centre = transform.translation.truncate();
    let visible = Rect::from_corners(centre - half, centre + half);
    let handle = HANDLE_PIXELS * orthographic.scale;

    let world = doc.document.world();
    let dragged: Vec<(campaign::FeatureId, Vec<CellPoint>)> =
        if dragging.what == Some(DragKind::Body) {
            gesture::dragged(world, &selection.features, dragging.offset())
        } else {
            Vec::new()
        };

    for (id, feature) in world.features() {
        let held = selection.features.contains(&id);
        let vertices: &[CellPoint] = dragged
            .iter()
            .find(|(dragged_id, _)| *dragged_id == id)
            .map(|(_, moved)| moved.as_slice())
            .unwrap_or_else(|| feature.geometry.vertices());

        let points: Vec<Vec2> = vertices
            .iter()
            .map(|vertex| view.cell_to_world(vertex.x, vertex.y))
            .collect();
        if !touches(&points, visible, handle) {
            continue;
        }

        let colour = Color::from(if held { css::GOLD } else { css::WHITE_SMOKE });
        let closed = matches!(feature.geometry, Geometry::Polygon(_));
        draw_shape(&mut gizmos, &points, closed, colour, handle, FEATURE_Z);

        if held {
            for (index, point) in points.iter().enumerate() {
                let selected = selection
                    .vertex
                    .is_some_and(|vertex| vertex.feature == id && vertex.index == index);
                let colour_of = if selected { css::ORANGE_RED } else { css::GOLD };
                mark(&mut gizmos, *point, handle, Color::from(colour_of), HANDLE_Z);
            }
        }
    }

    if let Some(draft) = drafting.draft.as_ref() {
        let mut points: Vec<Vec2> = draft
            .vertices()
            .iter()
            .map(|vertex| view.cell_to_world(vertex.x, vertex.y))
            .collect();
        if let Some(cell) = drafting.snap.map(|snap| snap.at).or(pointer.cell) {
            points.push(view.cell_to_world(cell.x, cell.y));
        }
        draw_shape(&mut gizmos, &points, false, Color::from(css::AQUAMARINE), handle, DRAFT_Z);
        for point in points.iter() {
            mark(&mut gizmos, *point, handle, Color::from(css::AQUAMARINE), DRAFT_Z);
        }
    }

    if active.drawing()
        && let Some(snap) = drafting.snap.filter(|snap| snap.landed())
    {
        let at = view.cell_to_world(snap.at.x, snap.at.y);
        mark(&mut gizmos, at, handle * 2.0, Color::from(css::SPRING_GREEN), DRAFT_Z);
    }

    if dragging.what == Some(DragKind::Box) {
        let from = view.cell_to_world(dragging.grabbed.x, dragging.grabbed.y);
        let to = view.cell_to_world(dragging.latest.x, dragging.latest.y);
        let rect = Rect::from_corners(from, to);
        gizmos.rect_2d(rect.center(), rect.size(), Color::from(css::AQUAMARINE));
    }
}

fn draw_shape(
    gizmos: &mut Gizmos,
    points: &[Vec2],
    closed: bool,
    colour: Color,
    handle: f32,
    depth: f32,
) {
    match points {
        [] => {}
        [only] => mark(gizmos, *only, handle, colour, depth),
        _ => {
            for pair in points.windows(2) {
                gizmos.line(pair[0].extend(depth), pair[1].extend(depth), colour);
            }
            if closed && points.len() > 2 {
                let last = points[points.len() - 1];
                gizmos.line(last.extend(depth), points[0].extend(depth), colour);
            }
        }
    }
}

fn mark(gizmos: &mut Gizmos, at: Vec2, radius: f32, colour: Color, depth: f32) {
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
