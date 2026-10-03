//! What one cell of the campaign is worth, as the GM sets it.
//!
//! The scale lands on `campaign.ron`, so it is committed on Enter rather than as it is
//! typed.

use bevy::feathers::controls::{FeathersTextInput, FeathersTextInputContainer};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::scene::CommandsSceneExt;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui_widgets::Activate;
use campaign::measure::DistanceUnit;

use crate::{CampaignChrome, OpenCampaign, StatusMessage};

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

/// Hangs the scale panel over the map.
pub fn build_scale_panel(mut commands: Commands, mut held: ResMut<ScaleFields>, open: Res<OpenCampaign>) {
    let manifest = open.0.manifest();
    held.units_per_cell = manifest.units_per_cell.to_string();
    commands.spawn_scene(panel());
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
/// Rescales the open campaign in place rather than opening a new one: systems are gated
/// on an [`OpenCampaign`] being *added*, so a re-inserted resource would run them again,
/// and re-opening would read the terrain again for the sake of one number.
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

fn panel() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            bottom: px(40),
            left: px(12),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: px(8),
            width: px(420),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        ScalePanel
        CampaignChrome
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
