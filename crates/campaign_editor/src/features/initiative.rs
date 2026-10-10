//! The initiative list beside the combat map on screen: one row per token in the order it
//! acts, with the roll typed beside it, and a press on a row selecting that token on the map.
//!
//! A roll is session state on the token, as the token itself is, so the order is asked of
//! [`campaign::token::Tokens::in_initiative_order`] and this module owns only the rows.

use bevy::feathers::controls::{ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui_widgets::Activate;
use campaign::token::initiative_of;

use crate::combat::CombatMaps;
use crate::dialog::replace_text;
use crate::features::token::TokenGesture;
use crate::{CampaignChrome, StatusMessage};

const PANEL_WIDTH: f32 = 150.0;

const ROLL_WIDTH: f32 = 36.0;

const SMALL_TEXT: f32 = 12.0;

/// The initiative panel's root.
#[derive(Component, Default, Clone)]
pub struct InitiativePanel;

/// The column the rows hang from, in the order the tokens act.
#[derive(Component, Default, Clone)]
pub struct InitiativeList;

/// One token's row, named for the token it lists.
#[derive(Component, Debug, Default, Clone, PartialEq, Eq)]
pub struct InitiativeRow {
    pub name: String,
}

/// The button in a row that selects its token.
#[derive(Component, Debug, Default, Clone, PartialEq, Eq)]
pub struct InitiativePick {
    pub name: String,
}

/// The text input in a row holding its token's roll.
#[derive(Component, Debug, Default, Clone, PartialEq, Eq)]
pub struct InitiativeRoll {
    pub name: String,
}

/// Hangs the initiative panel beside the combat maps panel, hidden until a combat map with a
/// token on it is on screen.
pub fn build_initiative_panel(mut commands: Commands) {
    commands.spawn_scene(panel());
}

/// Shows the panel only while the combat map on screen carries a token, keeps one row per
/// token in the order they act, and marks the selected token's row.
///
/// A row is kept, not rebuilt, while its token stays on the map, so a roll being typed keeps
/// its focus as the row moves.
pub fn show_initiative(
    mut commands: Commands,
    maps: Res<CombatMaps>,
    gesture: Res<TokenGesture>,
    mut panels: Query<&mut Node, With<InitiativePanel>>,
    lists: Query<(Entity, Option<&Children>), With<InitiativeList>>,
    rows: Query<(Entity, &InitiativeRow)>,
    mut picks: Query<(&InitiativePick, &mut ButtonVariant)>,
) {
    let order: Vec<&str> = maps
        .tokens_on_screen()
        .map(|(tokens, _)| tokens.in_initiative_order().into_iter().map(|token| token.name()).collect())
        .unwrap_or_default();

    let display = if order.is_empty() { Display::None } else { Display::Flex };
    for mut node in panels.iter_mut() {
        if node.display != display {
            node.display = display;
        }
    }

    for (list, children) in lists.iter() {
        let held: Vec<(Entity, &str)> = children
            .into_iter()
            .flatten()
            .filter_map(|child| rows.get(*child).ok())
            .map(|(entity, row)| (entity, row.name.as_str()))
            .collect();
        for (entity, name) in &held {
            if !order.contains(name) {
                commands.entity(*entity).despawn();
            }
        }
        let wanted: Vec<Entity> = order
            .iter()
            .map(|name| match held.iter().find(|(_, held)| held == name) {
                Some((entity, _)) => *entity,
                None => commands.spawn_scene(row(name)).id(),
            })
            .collect();
        let current: Vec<Entity> = children.into_iter().flatten().copied().collect();
        if current != wanted {
            commands.entity(list).replace_children(&wanted);
        }
    }

    for (pick, mut variant) in picks.iter_mut() {
        let wanted = if gesture.selected.as_deref() == Some(pick.name.as_str()) {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        };
        if *variant != wanted {
            *variant = wanted;
        }
    }
}

/// Writes each token's roll into its row's input, for a row just made, a map switched to, or
/// an input the GM has just left holding something that is not a roll.
///
/// The input being typed into is left alone.
pub fn seed_initiative_rolls(
    maps: Res<CombatMaps>,
    focus: Res<InputFocus>,
    mut inputs: Query<(Entity, Ref<InitiativeRoll>, &mut EditableText)>,
) {
    let Some((tokens, _)) = maps.tokens_on_screen() else {
        return;
    };
    for (entity, roll, mut text) in inputs.iter_mut() {
        if !(roll.is_added() || maps.is_changed() || focus.is_changed()) || focus.get() == Some(entity) {
            continue;
        }
        let Some(token) = tokens.get(&roll.name) else {
            continue;
        };
        let held = text.value().to_string();
        if initiative_of(&held).ok() != Some(token.initiative()) {
            replace_text(&mut text, &token.initiative().map(|roll| roll.to_string()).unwrap_or_default());
        }
    }
}

fn panel() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(96),
            right: {Val::Px(12.0 + PANEL_WIDTH + 8.0)},
            display: Display::None,
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            padding: px(6),
            width: px(PANEL_WIDTH),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        CampaignChrome
        InitiativePanel
        Children [
            Text("Initiative")
            TextFont { font_size: {bevy::text::FontSize::Px(SMALL_TEXT)} }
            ThemedText
            --
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
            }
            InitiativeList
        ]
    }
}

fn row(name: &str) -> impl Scene {
    let name = name.to_owned();
    let pick = name.clone();
    let roll = name.clone();
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(4),
        }
        InitiativeRow { name: {name.clone()} }
        Children [
            @FeathersTextInputContainer
            Node { width: {Val::Px(ROLL_WIDTH)} }
            Children [
                @FeathersTextInput
                InitiativeRoll { name: {roll.clone()} }
                on(|change: On<TextEditChange>,
                    inputs: Query<(&EditableText, &InitiativeRoll)>,
                    mut maps: ResMut<CombatMaps>,
                    mut status: ResMut<StatusMessage>| {
                    let Ok((text, input)) = inputs.get(change.event_target()) else {
                        return;
                    };
                    let roll = match initiative_of(&text.value().to_string()) {
                        Ok(roll) => roll,
                        Err(problem) => {
                            status.say(problem.to_string());
                            return;
                        }
                    };
                    let unchanged = maps
                        .tokens_on_screen()
                        .and_then(|(tokens, _)| tokens.get(&input.name))
                        .is_none_or(|token| token.initiative() == roll);
                    if unchanged {
                        return;
                    }
                    if let Some((tokens, _)) = maps.tokens_on_screen_mut()
                        && let Err(problem) = tokens.set_initiative(&input.name, roll)
                    {
                        status.say(problem.to_string());
                    }
                })
            ]
            --
            @FeathersButton {
                @caption: bsn! {
                    Text({pick.clone()})
                    TextFont { font_size: {bevy::text::FontSize::Px(SMALL_TEXT)} }
                    ThemedText
                },
            }
            Node { flex_grow: 1.0 }
            InitiativePick { name: {pick.clone()} }
            on(|activate: On<Activate>,
                picks: Query<&InitiativePick>,
                mut gesture: ResMut<TokenGesture>,
                mut focus: ResMut<InputFocus>| {
                let Ok(pick) = picks.get(activate.event_target()) else {
                    return;
                };
                gesture.drag = None;
                gesture.selected = Some(pick.name.clone());
                focus.clear();
            })
        ]
    }
}
