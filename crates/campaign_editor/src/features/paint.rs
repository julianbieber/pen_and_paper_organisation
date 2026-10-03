//! Turning a brush stroke on a dungeon's grid into one [`Edit`]. A combat map's strokes
//! share the gesture, [`advance_stroke`], and land in `features/combat.rs`.
//!
//! The stroke is held in a resource rather than a `Local` for the same reason a drag is:
//! Escape has to be able to abandon it, and the control socket has to be able to drive one
//! without a mouse. Three of the four brushes are decided on *release*, so the press is
//! gated on the pointer being over the map rather than the system being — a run condition
//! would abandon the stroke the moment the cursor crossed the tool strip and the release
//! would never be seen.
//!
//! One stroke is one [`Edit::PaintTiles`], applied on release, which is what makes a brush
//! stroke a single press of undo however many cells it covered. While the button is held
//! the stroke is drawn as a [`StrokePreview`] — the cells it would change, laid over the
//! chunks and never over the document — so painting answers the mouse as it moves without
//! putting an edit per frame on the undo stack.

use bevy::prelude::*;
use campaign::brush::{self, Brush};
use campaign::edit::Edit;
use campaign::feature::CellPoint;

use crate::StatusMessage;
use crate::combat::CombatMaps;
use crate::document::{self, WorldDoc};
use crate::features::PointerOverUi;
use crate::features::tool::ActiveTool;
use crate::map::chunks::PaintedCells;
use crate::map::pointer::MapPointer;

/// The stroke in progress, if the button is down.
///
/// `path` holds every cell the gesture has covered, interpolated between frames — a
/// freehand brush sampled once a frame at sixty frames a second skips several cells per
/// sweep of the mouse, and would draw a dotted line.
#[derive(Resource, Debug, Default)]
pub struct Stroking {
    pub path: Vec<(i64, i64)>,
}

impl Stroking {
    /// Whether a stroke is in progress.
    pub fn is_live(&self) -> bool {
        !self.path.is_empty()
    }

    /// Drop the stroke without applying it, as Escape and a document switch both do.
    pub fn abandon(&mut self) {
        self.path.clear();
    }

    /// The path the stroke would paint if the button were let go over `at` now.
    ///
    /// Empty when no stroke is live. A brush that only reads the corners gets the first
    /// cell and `at`; the freehand brush gets the path it has covered.
    pub fn preview_path(&self, at: Option<(i64, i64)>, brush: Brush) -> Vec<(i64, i64)> {
        let Some(&first) = self.path.first() else {
            return Vec::new();
        };
        if brush.tracks_the_path() {
            return self.path.clone();
        }
        vec![first, at.unwrap_or(first)]
    }
}

/// The cells the stroke in hand would change, as the tile index each would take.
///
/// Written by [`preview_stroke`] only when it differs, so the chunks are redrawn when the
/// stroke moves and not once a frame.
#[derive(Resource, Debug, Default)]
pub struct StrokePreview {
    pub cells: Vec<(u32, u32, u16)>,
    asked: Option<(usize, Option<(i64, i64)>, (i64, i64))>,
}

/// Works out what the stroke in hand would paint, on whichever grid is on screen.
///
/// Empty whenever no stroke is live, which is what clears the preview after a release,
/// Escape or a document switch. Recomputed only when the stroke's path or the pointer's
/// cell moved, so a flood fill is not run again every frame the mouse is still.
pub fn preview_stroke(
    pointer: Res<MapPointer>,
    active: Res<ActiveTool>,
    stroking: Res<Stroking>,
    doc: Option<Res<WorldDoc>>,
    combat: Option<Res<CombatMaps>>,
    mut preview: ResMut<StrokePreview>,
) {
    let path = match (stroking.is_live() && active.painting(), pointer.cell.map(cell_of)) {
        (true, at) => stroking.preview_path(at, active.brush),
        (false, _) => Vec::new(),
    };
    let Some((&first, &last)) = path.first().zip(path.last()) else {
        if !preview.cells.is_empty() || preview.asked.is_some() {
            preview.cells.clear();
            preview.asked = None;
        }
        return;
    };
    let asked = Some((path.len(), Some(first), last));
    if preview.asked == asked {
        return;
    }

    let cells = match combat.as_ref().and_then(|combat| combat.on_screen()) {
        Some(map) => as_indices(brush::cells(map.content().grid(), active.brush, &path, active.combat_tile)),
        None => match doc.as_ref().and_then(|doc| doc.document.world().grid()) {
            Some(grid) => as_indices(brush::cells(grid, active.brush, &path, active.tile)),
            None => Vec::new(),
        },
    };
    let moved = preview.cells != cells;
    let quiet = preview.bypass_change_detection();
    quiet.asked = asked;
    if moved {
        quiet.cells = cells;
        preview.set_changed();
    }
}

fn as_indices<T: campaign::grid::TileVocabulary>(
    changes: Vec<brush::TileChange<T>>,
) -> Vec<(u32, u32, u16)> {
    changes
        .into_iter()
        .map(|change| (change.x, change.y, change.tile.index()))
        .collect()
}

/// Turns the left button on a dungeon's grid into one paint edit.
///
/// Does nothing at all when the pointer is over no cell, so a stroke never begins at a
/// position that was never pointed at.
pub fn paint_tiles(
    buttons: Res<ButtonInput<MouseButton>>,
    pointer: Res<MapPointer>,
    over_ui: Res<PointerOverUi>,
    active: Res<ActiveTool>,
    mut stroking: ResMut<Stroking>,
    mut doc: ResMut<WorldDoc>,
    mut painted: ResMut<PaintedCells>,
    mut status: ResMut<StatusMessage>,
) {
    let Some(path) = advance_stroke(
        &buttons,
        pointer.cell.map(cell_of),
        over_ui.0,
        active.brush,
        &mut stroking,
    ) else {
        return;
    };

    let Some(grid) = doc.document.world().grid() else {
        return;
    };
    let changes = brush::cells(grid, active.brush, &path, active.tile);
    if changes.is_empty() {
        return;
    }

    let count = changes.len();
    if document::apply(
        &mut doc,
        &mut status,
        Edit::PaintTiles {
            changes: changes.clone(),
        },
    ) {
        painted.cells.extend(changes.iter().map(|change| (change.x, change.y)));
        status.say(format!("painted {count} cell(s)"));
    }
}

/// Carries the stroke in `stroking` one frame on, and hands back every cell it covered on
/// the frame the left button is released.
///
/// `at` is the cell under the pointer, or `None` when it is over no cell; a stroke never
/// begins there, nor where `over_ui` says the press landed on the interface. `brush` decides
/// whether the cells between frames are part of the path. `None` on every other frame, and
/// the stroke is gone from `stroking` once its path has been handed back.
pub fn advance_stroke(
    buttons: &ButtonInput<MouseButton>,
    at: Option<(i64, i64)>,
    over_ui: bool,
    brush: Brush,
    stroking: &mut Stroking,
) -> Option<Vec<(i64, i64)>> {
    if buttons.just_pressed(MouseButton::Left) && !over_ui
        && let Some(at) = at
    {
        stroking.path = vec![at];
    }

    if !stroking.is_live() {
        return None;
    }

    if buttons.pressed(MouseButton::Left)
        && brush.tracks_the_path()
        && let Some(at) = at
        && stroking.path.last() != Some(&at)
    {
        let from = *stroking.path.last().expect("a live stroke has a first cell");
        let mut between = brush::line(from, at);
        between.remove(0);
        stroking.path.extend(between);
    }

    if !buttons.just_released(MouseButton::Left) {
        return None;
    }

    let mut path = std::mem::take(&mut stroking.path);
    if let Some(at) = at
        && path.last() != Some(&at)
    {
        path.push(at);
    }
    Some(path)
}

/// Whether the paint tool is in hand.
pub fn the_paint_tool_is_active(active: Res<ActiveTool>) -> bool {
    active.painting()
}

/// Whether the open document is one a brush can be used on.
pub fn a_grid_is_open(doc: Option<Res<WorldDoc>>) -> bool {
    doc.is_some_and(|doc| doc.document.world().grid().is_some())
}

/// The whole cell a position in cells lies in, rounding each axis down.
pub(crate) fn cell_of(cell: CellPoint) -> (i64, i64) {
    (cell.x.floor() as i64, cell.y.floor() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::tool::Tool;
    use campaign::tiles::CombatTile;

    fn app_painting(brush: Brush) -> App {
        let mut app = App::new();
        let mut maps = CombatMaps::default();
        maps.open(
            campaign::CombatMap::new("Ford", 10, 10).expect("a 10x10 map is valid"),
            "ford.ron".to_owned(),
            None,
        );
        app.insert_resource(maps)
            .insert_resource(ActiveTool {
                tool: Tool::Paint,
                brush,
                combat_tile: CombatTile::Dirt,
                ..default()
            })
            .init_resource::<Stroking>()
            .init_resource::<StrokePreview>()
            .init_resource::<MapPointer>()
            .add_systems(Update, preview_stroke);
        app
    }

    fn point_at(app: &mut App, x: f32, y: f32) {
        app.world_mut().resource_mut::<MapPointer>().cell = Some(CellPoint::new(x, y));
    }

    fn previewed(app: &App) -> Vec<(u32, u32)> {
        let mut cells: Vec<(u32, u32)> = app
            .world()
            .resource::<StrokePreview>()
            .cells
            .iter()
            .map(|&(x, y, _)| (x, y))
            .collect();
        cells.sort_unstable();
        cells
    }

    // The point of the preview: a stroke shows on the map while the button is still down,
    // and follows the pointer, rather than appearing only on release.
    #[test]
    fn a_held_stroke_is_previewed_as_the_pointer_moves() {
        let mut app = app_painting(Brush::Rectangle);
        app.world_mut().resource_mut::<Stroking>().path = vec![(1, 1)];
        point_at(&mut app, 2.5, 1.5);
        app.update();
        assert_eq!(previewed(&app), vec![(1, 1), (2, 1)]);

        point_at(&mut app, 2.5, 2.5);
        app.update();
        assert_eq!(previewed(&app), vec![(1, 1), (1, 2), (2, 1), (2, 2)]);
    }

    // Once the stroke is gone — released, or abandoned with Escape — the preview must go
    // with it, or the map keeps showing tiles the document never got.
    #[test]
    fn the_preview_empties_when_the_stroke_ends() {
        let mut app = app_painting(Brush::Freehand);
        app.world_mut().resource_mut::<Stroking>().path = vec![(1, 1), (2, 1)];
        point_at(&mut app, 2.5, 1.5);
        app.update();
        assert_eq!(previewed(&app).len(), 2);

        app.world_mut().resource_mut::<Stroking>().abandon();
        app.update();
        assert!(previewed(&app).is_empty());
    }
}
