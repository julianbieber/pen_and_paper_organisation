//! Turning a brush stroke on a dungeon's grid into one [`Edit`].
//!
//! The stroke is held in a resource rather than a `Local` for the same reason a drag is:
//! Escape has to be able to abandon it, and the control socket has to be able to drive one
//! without a mouse. Three of the four brushes are decided on *release*, so the press is
//! gated on the pointer being over the map rather than the system being — a run condition
//! would abandon the stroke the moment the cursor crossed the tool strip and the release
//! would never be seen.
//!
//! One stroke is one [`Edit::PaintTiles`], applied on release, which is what makes a brush
//! stroke a single press of undo however many cells it covered. Nothing is applied while
//! the button is held, so a stroke also costs one change-detection wake rather than one a
//! frame.

use bevy::prelude::*;
use campaign::brush;
use campaign::edit::Edit;
use campaign::feature::CellPoint;

use crate::StatusMessage;
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
    let at = pointer.cell.map(cell_of);

    if buttons.just_pressed(MouseButton::Left) && !over_ui.0
        && let Some(at) = at
    {
        stroking.path = vec![at];
    }

    if !stroking.is_live() {
        return;
    }

    if buttons.pressed(MouseButton::Left)
        && active.brush.tracks_the_path()
        && let Some(at) = at
        && stroking.path.last() != Some(&at)
    {
        let from = *stroking.path.last().expect("a live stroke has a first cell");
        let mut between = brush::line(from, at);
        between.remove(0);
        stroking.path.extend(between);
    }

    if !buttons.just_released(MouseButton::Left) {
        return;
    }

    let mut path = std::mem::take(&mut stroking.path);
    if let Some(at) = at
        && path.last() != Some(&at)
    {
        path.push(at);
    }

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
        painted.0.extend(changes);
        status.say(format!("painted {count} cell(s)"));
    }
}

/// Whether the paint tool is in hand.
pub fn the_paint_tool_is_active(active: Res<ActiveTool>) -> bool {
    active.painting()
}

/// Whether the open document is one a brush can be used on.
pub fn a_grid_is_open(doc: Option<Res<WorldDoc>>) -> bool {
    doc.is_some_and(|doc| doc.document.world().grid().is_some())
}

fn cell_of(cell: CellPoint) -> (i64, i64) {
    (cell.x.floor() as i64, cell.y.floor() as i64)
}
