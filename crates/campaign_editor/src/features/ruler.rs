//! The measurement in hand: the points the GM clicked, and the figures shown for them.
//!
//! A measurement is session state and nothing else. It becomes no [`Edit`](campaign::Edit),
//! reaches no document and is never saved, so leaving the tool, switching document or
//! pressing Escape simply forgets it. That is why nothing here is undoable and why the
//! resource is cleared from wherever a gesture is abandoned rather than defending itself.
//!
//! What a cell is worth, how long a path is and how that reads as a figure are all asked
//! of [`campaign::measure`]. This module owns the clicks and the pixels.

use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::scene::CommandsSceneExt;
use campaign::feature::CellPoint;
use campaign::measure::{self, CellWorth};

use crate::document::WorldDoc;
use crate::features::pens::MeasurePens;
use crate::features::tool::ActiveTool;
use crate::features::PointerOverUi;
use crate::map::backdrop::Backdrop;
use crate::map::view::FEATURE_Z;
use crate::{OpenCampaign, StatusMessage};

const MEASURE_Z: f32 = FEATURE_Z + 0.3;

/// The most points one measurement may hold.
///
/// A measurement is a handful of clicks; a cap this far above that exists so a stuck
/// button cannot grow the path without bound.
pub const MAX_MEASURE_POINTS: usize = 512;

/// How fast the party travels, in the document's own unit per day.
///
/// Session state: a pace is how the GM is playing today, not something a campaign
/// directory carries between machines. Landed from the slider in
/// [`map::scale`](crate::map::scale).
#[derive(Resource, Debug, Clone, Copy)]
pub struct TravelSpeed {
    pub units_per_day: f64,
}

/// Where the travel-speed slider starts.
pub const DEFAULT_UNITS_PER_DAY: f64 = 30.0;

/// The slowest pace the slider offers.
pub const MIN_UNITS_PER_DAY: f64 = 20.0;

/// The fastest pace the slider offers.
///
/// The range is bounded away from zero, so a pace is always a pace and no figure derived
/// from one is ever divided by nothing.
pub const MAX_UNITS_PER_DAY: f64 = 40.0;

impl Default for TravelSpeed {
    fn default() -> Self {
        Self {
            units_per_day: DEFAULT_UNITS_PER_DAY,
        }
    }
}

/// The measurement in hand.
///
/// `finished` is what stops the path following the pointer once the GM has pressed Enter,
/// so a measurement can be read without holding the mouse still.
#[derive(Resource, Debug, Default, Clone)]
pub struct Ruler {
    points: Vec<CellPoint>,
    finished: bool,
}

impl Ruler {
    /// Whether any measurement is in hand, finished or not.
    pub fn is_live(&self) -> bool {
        !self.points.is_empty()
    }

    /// Whether the measurement in hand has been finished with Enter.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Forget the measurement entirely.
    ///
    /// What leaving the tool, switching document and Escape all do. Safe to call when
    /// there is nothing in hand.
    pub fn clear(&mut self) {
        self.points.clear();
        self.finished = false;
    }

    /// The path as it is drawn: the points clicked, and while unfinished, `at` as one more.
    ///
    /// The pointer's own position is never stored, because a resource rewritten every frame
    /// the mouse moves is a resource every change gate downstream stops trusting.
    pub fn path(&self, at: Option<CellPoint>) -> Vec<CellPoint> {
        let mut path = self.points.clone();
        if !self.finished && let Some(at) = at {
            path.push(at);
        }
        path
    }
}

/// Whether the measure tool is the one in hand.
pub fn the_measure_tool_is_active(tool: Res<ActiveTool>) -> bool {
    tool.measuring()
}

/// Turns clicks into a measurement path, and Enter into finishing one.
///
/// Nothing here reaches the document, the undo stack or a saved file. Escape is not read
/// here: it is arbitrated in [`keys`](crate::features::keys) with every other cancellation,
/// because a key press is seen by every system that looks for it and none of them consumes
/// it.
pub fn run_ruler(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    pointer: Res<crate::map::pointer::MapPointer>,
    over_ui: Res<PointerOverUi>,
    mut ruler: ResMut<Ruler>,
    mut status: ResMut<StatusMessage>,
) {
    if buttons.just_pressed(MouseButton::Left)
        && !over_ui.0
        && let Some(at) = pointer.cell
    {
        if ruler.finished {
            ruler.clear();
        }
        if ruler.points.len() >= MAX_MEASURE_POINTS {
            status.say(format!(
                "a measurement holds at most {MAX_MEASURE_POINTS} points"
            ));
        } else {
            ruler.points.push(at);
        }
    }

    if keys.just_pressed(KeyCode::Enter) && ruler.points.len() >= 2 {
        ruler.finished = true;
    }
}

/// Strokes the measurement and the line across it, and writes the figures.
///
/// Runs after the pens are sized, since a gizmo's width is written by that system and read
/// by this one.
pub fn draw_ruler(
    ruler: Res<Ruler>,
    pointer: Res<crate::map::pointer::MapPointer>,
    backdrop: Res<Backdrop>,
    doc: Res<WorldDoc>,
    open: Res<OpenCampaign>,
    speed: Res<TravelSpeed>,
    mut pens: MeasurePens,
    mut readouts: Query<&mut Text, With<RulerReadout>>,
) {
    let path = ruler.path(pointer.cell);
    let worth = measure::worth_of(doc.document.world(), open.0.manifest());
    let measured = measure::measure_of(&path, false, &worth, speed.units_per_day);

    if let Some(measured) = measured {
        let view = backdrop.view;
        let world: Vec<Vec2> = path
            .iter()
            .map(|point| view.cell_to_world(point.x, point.y))
            .collect();

        for pair in world.windows(2) {
            pens.path.line(
                pair[0].extend(MEASURE_Z),
                pair[1].extend(MEASURE_Z),
                Color::WHITE,
            );
        }
        if let (Some(first), Some(last)) = (world.first(), world.last())
            && measured.straight.is_some()
        {
            pens.ghost.line(
                first.extend(MEASURE_Z),
                last.extend(MEASURE_Z),
                Color::srgb(0.8, 0.8, 0.8),
            );
        }
    }

    let shown = readout_text(measured.as_ref(), &worth);
    for mut text in readouts.iter_mut() {
        if text.0 != shown {
            text.0 = shown.clone();
        }
    }
}

/// The line the measurement's figures are written on.
#[derive(Component, Default, Clone)]
pub struct RulerReadout;

/// Hangs the measurement's readout over the map.
pub fn build_ruler_readout(mut commands: Commands) {
    commands.spawn_scene(readout());
}

fn readout() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            padding: px(8),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            (Text("") ThemedText RulerReadout)
        ]
    }
}

fn readout_text(measured: Option<&measure::Measurement>, worth: &CellWorth) -> String {
    let Some(measured) = measured else {
        return "click two places to measure".to_owned();
    };
    let mut shown = measure::figure(measured.path.distance, worth.unit());
    if let Some(days) = measured.path.days {
        shown.push_str(&format!(" ({})", measure::duration(days)));
    }
    if let Some(straight) = measured.straight {
        shown.push_str(&format!(
            "  |  {} direct",
            measure::figure(straight.distance, worth.unit())
        ));
        if let Some(days) = straight.days {
            shown.push_str(&format!(" ({})", measure::duration(days)));
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> CellPoint {
        CellPoint::new(x, y)
    }

    // The pointer's live position is drawn but never stored, or the resource would be
    // marked changed on every frame the mouse moves.
    #[test]
    fn the_pointer_is_part_of_the_path_but_not_of_the_ruler() {
        let mut ruler = Ruler::default();
        ruler.points.push(at(0.0, 0.0));
        assert_eq!(ruler.path(None).len(), 1);
        assert_eq!(ruler.path(Some(at(3.0, 4.0))).len(), 2);
    }

    // Once finished, the measurement stops following the pointer so it can be read.
    #[test]
    fn a_finished_measurement_ignores_the_pointer() {
        let mut ruler = Ruler::default();
        ruler.points.push(at(0.0, 0.0));
        ruler.points.push(at(1.0, 0.0));
        ruler.finished = true;
        assert_eq!(ruler.path(Some(at(9.0, 9.0))).len(), 2);
    }

    // Clearing has to reset the flag too, or the next measurement restarts on its second
    // click and can never hold two points.
    #[test]
    fn clearing_forgets_that_a_measurement_was_finished() {
        let mut ruler = Ruler::default();
        ruler.points.push(at(0.0, 0.0));
        ruler.finished = true;
        ruler.clear();
        assert!(!ruler.is_finished());
        assert!(!ruler.is_live());
    }
}
