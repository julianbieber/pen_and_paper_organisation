//! What one cell of the campaign is worth, and how fast the party travels, as the GM sets
//! them.
//!
//! A root of its own rather than a row on the river panel: that panel is taken off screen
//! inside a dungeon, and both of these still say something there — the scale bar and the
//! ruler are drawn on a dungeon too.
//!
//! The scale lands on `campaign.ron`, so it is committed on Enter or on leaving the field
//! rather than as it is typed, exactly as a feature's label is. The pace lands on a
//! resource and is never written anywhere.

use bevy::feathers::controls::{FeathersSlider, FeathersTextInput, FeathersTextInputContainer};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::scene::CommandsSceneExt;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui_widgets::{Activate, SliderValue, slider_self_update};
use campaign::measure::DistanceUnit;

use crate::features::ruler::{
    DEFAULT_UNITS_PER_DAY, MAX_UNITS_PER_DAY, MIN_UNITS_PER_DAY, TravelSpeed,
};
use crate::{OpenCampaign, StatusMessage};

/// The field the campaign's scale is typed into.
#[derive(Component, Default, Clone)]
pub struct ScaleField;

/// A button choosing the campaign's unit.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct UnitButton {
    pub unit: DistanceUnit,
}

impl Default for UnitButton {
    fn default() -> Self {
        Self {
            unit: DistanceUnit::Kilometres,
        }
    }
}

/// The slider the party's pace is read from.
#[derive(Component, Default, Clone)]
pub struct SpeedSlider;

/// The panel's root.
#[derive(Component, Default, Clone)]
pub struct ScalePanel;

/// What the GM has typed but not yet landed, and which unit they chose.
///
/// Held rather than applied because a text field reports every character and landing per
/// character would rewrite `campaign.ron` per keystroke.
#[derive(Resource, Debug, Default)]
pub struct ScaleFields {
    pub units_per_cell: String,
    pub unit: Option<DistanceUnit>,
    pub asked: bool,
}

/// Hangs the scale and pace panel over the map.
pub fn build_scale_panel(mut commands: Commands, mut held: ResMut<ScaleFields>, open: Res<OpenCampaign>) {
    let manifest = open.0.manifest();
    held.units_per_cell = manifest.units_per_cell.to_string();
    commands.spawn_scene(panel(DEFAULT_UNITS_PER_DAY as f32));
}

/// Asks for the typed scale to be landed, when the GM presses Enter in the field.
///
/// Enter rather than every keystroke, because a text field reports every character and
/// landing per character would rewrite `campaign.ron` once per letter.
pub fn commit_scale(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    fields: Query<(), With<ScaleField>>,
    mut held: ResMut<ScaleFields>,
) {
    if !keys.just_pressed(KeyCode::Enter) && !keys.just_pressed(KeyCode::NumpadEnter) {
        return;
    }
    let Some(entity) = focus.get() else {
        return;
    };
    if fields.contains(entity) {
        held.asked = true;
    }
}

/// Lands a scale or a unit the GM chose into `campaign.ron`.
///
/// Rescales the open campaign in place rather than opening a new one: three systems are
/// gated on an [`OpenCampaign`] being *added*, so a re-inserted resource would build a
/// second notes panel, and re-opening would read the terrain again for the sake of one
/// number.
pub fn land_campaign_scale(
    mut held: ResMut<ScaleFields>,
    mut open: ResMut<OpenCampaign>,
    mut status: ResMut<StatusMessage>,
) {
    if !std::mem::take(&mut held.asked) {
        return;
    }
    let manifest = open.0.manifest();
    let units_per_cell = match held.units_per_cell.trim().parse::<f64>() {
        Ok(parsed) => parsed,
        Err(_) if held.units_per_cell.trim().is_empty() => manifest.units_per_cell,
        Err(error) => {
            status.say(format!("that is not a scale: {error}"));
            return;
        }
    };
    let unit = held
        .unit
        .map(|unit| unit.label().to_owned())
        .unwrap_or_else(|| manifest.unit.clone());

    match open.0.rescale(units_per_cell, &unit) {
        Ok(()) => {
            let manifest = open.0.manifest();
            status.say(format!(
                "one cell is {} {}",
                manifest.units_per_cell, manifest.unit
            ));
        }
        Err(problem) => status.say(problem.to_string()),
    }
}

/// Lands a moved pace slider on [`TravelSpeed`].
///
/// Writes only where the value differs, as
/// [`land_threshold`](crate::map::panel::land_threshold) does: an unconditional write marks
/// the resource changed every frame, and the panel reading it would then re-lay-out its
/// lines for the life of the process.
pub fn land_speed(sliders: Query<&SliderValue, With<SpeedSlider>>, mut speed: ResMut<TravelSpeed>) {
    for value in sliders.iter() {
        let asked = f64::from(value.0);
        if speed.units_per_day != asked {
            speed.units_per_day = asked;
        }
    }
}

fn panel(pace: f32) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            bottom: px(160),
            left: px(12),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: px(8),
            width: px(420),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        ScalePanel
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                }
                Children [
                    (Text("One cell is") ThemedText),
                    (
                        @FeathersTextInputContainer
                        Children [
                            (
                                @FeathersTextInput
                                ScaleField
                                on(|change: On<TextEditChange>,
                                    texts: Query<&EditableText>,
                                    mut held: ResMut<ScaleFields>| {
                                    if let Ok(text) = texts.get(change.event_target()) {
                                        held.units_per_cell = text.value().to_string();
                                    }
                                })
                            )
                        ]
                    )
                ]
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                }
                Children [
                    unit_button(DistanceUnit::Feet),
                    unit_button(DistanceUnit::Metres),
                    unit_button(DistanceUnit::Kilometres),
                    unit_button(DistanceUnit::Miles),
                    unit_button(DistanceUnit::Leagues)
                ]
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                }
                Children [
                    (Text("A day's travel") ThemedText),
                    (
                        @FeathersSlider {
                            @value: {pace},
                            @min: {MIN_UNITS_PER_DAY as f32},
                            @max: {MAX_UNITS_PER_DAY as f32},
                        }
                        SpeedSlider
                        on(slider_self_update)
                    )
                ]
            )
        ]
    }
}

fn unit_button(unit: DistanceUnit) -> impl Scene {
    bsn! {
        @bevy::feathers::controls::FeathersButton {
            @caption: bsn! { Text({unit.label().to_string()}) ThemedText },
        }
        UnitButton { unit: {unit} }
        on(|activate: On<Activate>,
            buttons: Query<&UnitButton>,
            mut held: ResMut<ScaleFields>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            held.unit = Some(button.unit);
            held.asked = true;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct Probe {
        moved: bool,
    }

    fn probe(speed: Res<TravelSpeed>, mut seen: ResMut<Probe>) {
        seen.moved = speed.is_changed();
    }

    fn app_with(value: f32) -> App {
        let mut app = App::new();
        app.init_resource::<TravelSpeed>()
            .init_resource::<Probe>()
            .add_systems(Update, (land_speed, probe).chain());
        app.world_mut().spawn((SliderValue(value), SpeedSlider));
        app
    }

    // A still slider must not mark the resource changed, or every panel reading the pace
    // re-lays-out its lines every frame for the life of the process.
    #[test]
    fn a_slider_that_has_not_moved_does_not_mark_the_pace_changed() {
        let mut app = app_with(DEFAULT_UNITS_PER_DAY as f32);
        app.update();
        app.update();
        assert!(!app.world().resource::<Probe>().moved);
    }

    // A moved slider must reach the resource, or the figures never follow the pace.
    #[test]
    fn a_moved_slider_lands_on_the_pace() {
        let mut app = app_with(MAX_UNITS_PER_DAY as f32);
        app.update();
        assert_eq!(
            app.world().resource::<TravelSpeed>().units_per_day,
            MAX_UNITS_PER_DAY
        );
    }
}
