//! How the GM handles the tokens on the combat map on screen, and how those tokens look.
//!
//! A token is session state and nothing else, as a measurement is. It becomes no
//! [`CombatEdit`](campaign::CombatEdit), sits on no undo stack and is never saved, so moving
//! one back is dragging it back. The tokens themselves live on their map in
//! [`CombatMaps`], which is what keeps them while the map is parked; only what the GM has
//! typed, chosen and is holding lives here, and the holding is cleared wherever every other
//! gesture is.
//!
//! What a token is called, where it lands, what it covers and which one a cell picks are all
//! asked of [`campaign::token`]. This module owns the clicks, the panel and the pixels.

use bevy::camera::Projection;
use bevy::color::palettes::css;
use bevy::feathers::controls::{ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use campaign::token::{MAX_TOKEN_SIZE, MIN_TOKEN_SIZE};

use crate::combat::CombatMaps;
use crate::features::PointerOverUi;
use crate::features::paint::cell_of;
use crate::features::pens::{TOKEN_LABEL_PIXELS, TokenPens};
use crate::features::tool::ActiveTool;
use crate::map::backdrop::Backdrop;
use crate::map::camera::MapCamera;
use crate::map::pointer::MapPointer;
use crate::map::view::FEATURE_Z;
use crate::{CampaignChrome, StatusMessage};

const TOKEN_Z: f32 = FEATURE_Z + 0.4;

/// What the token tool places: the name typed, the size chosen, and the name typed for a
/// rename.
///
/// Session state, reset when the campaign closes.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct TokenFields {
    pub name: String,
    /// Cells along a side, from [`MIN_TOKEN_SIZE`] to [`MAX_TOKEN_SIZE`].
    pub size: u8,
    pub rename: String,
}

impl Default for TokenFields {
    fn default() -> Self {
        Self {
            name: String::new(),
            size: MIN_TOKEN_SIZE,
            rename: String::new(),
        }
    }
}

/// A token being dragged, in whole cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenDrag {
    pub name: String,
    /// The cell the press landed on.
    pub grabbed: (i64, i64),
    /// The cell the pointer is over now.
    pub latest: (i64, i64),
}

/// Which token is selected on the map on screen, and the drag in hand.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct TokenGesture {
    /// The name of the selected token, which the panel renames and deletes.
    pub selected: Option<String>,
    pub drag: Option<TokenDrag>,
}

impl TokenGesture {
    /// Forget the drag without landing it, keeping the selection.
    pub fn cancel_drag(&mut self) {
        self.drag = None;
    }

    /// Forget the drag and the selection.
    pub fn clear(&mut self) {
        self.drag = None;
        self.selected = None;
    }
}

/// What the panel or the socket asked of the tokens, consumed by [`apply_token_intent`].
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TokenIntent {
    #[default]
    Nothing,
    /// Rename the selected token to [`TokenFields::rename`].
    Rename,
    /// Remove the selected token.
    Delete,
    /// Remove every token on the map on screen.
    Clear,
}

/// The token panel's root.
#[derive(Component, Default, Clone)]
pub struct TokenPanel;

/// Which of [`TokenFields`] a text input writes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TokenFieldKind {
    #[default]
    Name,
    Rename,
}

/// A text input in the token panel.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub struct TokenField {
    pub kind: TokenFieldKind,
}

/// A button choosing the size the token tool places.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub struct TokenSizeButton {
    pub size: u8,
}

/// The line naming the selected token.
#[derive(Component, Default, Clone)]
pub struct SelectedTokenCaption;

/// A button that acts on the selected token, disabled while none is.
#[derive(Component, Default, Clone)]
pub struct NeedsSelectedToken;

/// Whether the token tool is the one in hand.
pub fn the_token_tool_is_active(active: Res<ActiveTool>) -> bool {
    active.placing_tokens()
}

/// Places a token on the combat map on screen at each press of the left button, named and
/// sized from [`TokenFields`], and selects it.
///
/// A press over the UI places nothing. A refusal is said on the status line.
pub fn place_tokens(
    buttons: Res<ButtonInput<MouseButton>>,
    pointer: Res<MapPointer>,
    over_ui: Res<PointerOverUi>,
    fields: Res<TokenFields>,
    mut gesture: ResMut<TokenGesture>,
    mut maps: ResMut<CombatMaps>,
    mut status: ResMut<StatusMessage>,
) {
    if !buttons.just_pressed(MouseButton::Left) || over_ui.0 {
        return;
    }
    let (Some(cell), Some((tokens, extent))) = (pointer.cell, maps.tokens_on_screen_mut()) else {
        return;
    };
    match tokens.place(&fields.name, fields.size, cell_of(cell), extent) {
        Ok(token) => {
            status.say(format!("placed {}", token.name()));
            gesture.drag = None;
            gesture.selected = Some(token.name().to_owned());
        }
        Err(problem) => status.say(problem.to_string()),
    }
}

/// Selects the topmost token under a press on the combat map on screen and carries a drag
/// of it until release, when it lands on whole cells.
///
/// A press on no token selects none. A press over the UI does nothing, but a release over
/// it still lands the drag.
pub fn move_tokens(
    buttons: Res<ButtonInput<MouseButton>>,
    pointer: Res<MapPointer>,
    over_ui: Res<PointerOverUi>,
    mut gesture: ResMut<TokenGesture>,
    mut maps: ResMut<CombatMaps>,
    mut status: ResMut<StatusMessage>,
) {
    let cell = pointer.cell.map(cell_of);

    if buttons.just_pressed(MouseButton::Left) && !over_ui.0 {
        let Some(cell) = cell else {
            return;
        };
        let hit = maps
            .tokens_on_screen()
            .and_then(|(tokens, _)| tokens.topmost_at(cell.0, cell.1))
            .map(|token| token.name().to_owned());
        gesture.drag = hit.clone().map(|name| TokenDrag {
            name,
            grabbed: cell,
            latest: cell,
        });
        gesture.selected = hit;
        return;
    }

    if let (Some(cell), Some(drag)) = (cell, gesture.drag.as_ref())
        && drag.latest != cell
    {
        gesture.drag.as_mut().expect("a drag was just read").latest = cell;
    }

    if !buttons.just_released(MouseButton::Left) {
        return;
    }
    let Some(drag) = gesture.drag.take() else {
        return;
    };
    let Some((tokens, extent)) = maps.tokens_on_screen_mut() else {
        return;
    };
    match tokens.move_by(&drag.name, drag.grabbed, drag.latest, extent) {
        Ok(true) => {
            if let Some(token) = tokens.get(&drag.name) {
                status.say(format!("moved {} to ({}, {})", token.name(), token.x(), token.y()));
            }
        }
        Ok(false) => {}
        Err(problem) => status.say(problem.to_string()),
    }
}

/// Renames, deletes or clears as [`TokenIntent`] asks, on the combat map on screen, and says
/// the outcome on the status line.
///
/// A rename or a delete with no token selected, or any intent with no combat map on screen,
/// is said and does nothing.
pub fn apply_token_intent(
    mut intent: ResMut<TokenIntent>,
    fields: Res<TokenFields>,
    mut gesture: ResMut<TokenGesture>,
    mut maps: ResMut<CombatMaps>,
    mut status: ResMut<StatusMessage>,
) {
    let asked = std::mem::take(&mut *intent);
    if asked == TokenIntent::Nothing {
        return;
    }
    let Some((tokens, _)) = maps.tokens_on_screen_mut() else {
        status.say("no combat map is on screen");
        return;
    };
    if asked == TokenIntent::Clear {
        let count = tokens.len();
        tokens.clear();
        gesture.clear();
        status.say(format!("cleared {count} token(s)"));
        return;
    }
    let Some(selected) = gesture.selected.clone() else {
        status.say("select a token first");
        return;
    };
    match asked {
        TokenIntent::Rename => match tokens.rename(&selected, &fields.rename) {
            Ok(name) => {
                status.say(format!("renamed {selected} to {name}"));
                gesture.selected = Some(name);
            }
            Err(problem) => status.say(problem.to_string()),
        },
        TokenIntent::Delete => match tokens.remove(&selected) {
            Ok(token) => {
                status.say(format!("deleted {}", token.name()));
                gesture.clear();
            }
            Err(problem) => status.say(problem.to_string()),
        },
        TokenIntent::Nothing | TokenIntent::Clear => {}
    }
}

/// Takes the keyboard focus off a text field when the left button is pressed on the map, so
/// the press that follows typing a token's name reaches the map rather than being held back
/// while the field has focus.
pub fn release_focus_on_map_press(
    buttons: Res<ButtonInput<MouseButton>>,
    over_ui: Res<PointerOverUi>,
    mut focus: ResMut<InputFocus>,
    fields: Query<(), With<EditableText>>,
) {
    if buttons.just_pressed(MouseButton::Left)
        && !over_ui.0
        && focus.get().is_some_and(|entity| fields.contains(entity))
    {
        focus.clear();
    }
}

/// Strokes every token on the combat map on screen: the square it covers, a ring, and its
/// name at a fixed size on screen, the selected token in gold and a dragged one where it
/// would land.
pub fn draw_tokens(
    backdrop: Res<Backdrop>,
    maps: Res<CombatMaps>,
    gesture: Res<TokenGesture>,
    camera: Single<&Projection, With<MapCamera>>,
    mut pens: TokenPens,
) {
    let Some((tokens, extent)) = maps.tokens_on_screen() else {
        return;
    };
    let Projection::Orthographic(orthographic) = *camera else {
        return;
    };
    let view = backdrop.view;
    let lettering = TOKEN_LABEL_PIXELS * orthographic.scale;

    for token in tokens.iter() {
        let dragged = gesture
            .drag
            .as_ref()
            .filter(|drag| drag.name == token.name())
            .and_then(|drag| tokens.dragged(&drag.name, drag.grabbed, drag.latest, extent));
        let (x, y) = dragged.unwrap_or((token.x(), token.y()));
        let side = f32::from(token.size()) * view.cell_size;
        let centre = view.cell_corner_to_world(x as f32, y as f32) + Vec2::new(side / 2.0, -side / 2.0);
        let selected = gesture.selected.as_deref() == Some(token.name());
        let colour = if selected { Color::from(css::GOLD) } else { Color::WHITE };

        pens.ring.rect_2d(centre, Vec2::splat(side), colour.with_alpha(0.5));
        pens.ring.circle_2d(centre, side * 0.42, colour);
        let at = Isometry3d::from_translation(centre.extend(TOKEN_Z));
        pens.shadow.text(at, token.name(), lettering, Vec2::ZERO, Color::BLACK.with_alpha(0.8));
        pens.label.text(at, token.name(), lettering, Vec2::ZERO, Color::WHITE);
    }
}

/// Hangs the token panel over the map, hidden until a combat map is on screen.
pub fn build_token_panel(mut commands: Commands) {
    commands.spawn_scene(panel());
}

/// Shows the token panel only while a combat map is on screen, marks the size the token
/// tool places, names the selected token, and enables *Rename* and *Delete* only while one
/// is selected.
pub fn show_token_panel(
    mut commands: Commands,
    maps: Res<CombatMaps>,
    fields: Res<TokenFields>,
    gesture: Res<TokenGesture>,
    mut panels: Query<&mut Node, With<TokenPanel>>,
    mut sizes: Query<(&TokenSizeButton, &mut ButtonVariant)>,
    mut captions: Query<&mut Text, With<SelectedTokenCaption>>,
    needing: Query<(Entity, Has<InteractionDisabled>), With<NeedsSelectedToken>>,
) {
    let display = if maps.is_on_screen() { Display::Flex } else { Display::None };
    for mut node in panels.iter_mut() {
        if node.display != display {
            node.display = display;
        }
    }

    for (button, mut variant) in sizes.iter_mut() {
        let wanted = if button.size == fields.size {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        };
        if *variant != wanted {
            *variant = wanted;
        }
    }

    let selected = gesture
        .selected
        .as_deref()
        .and_then(|name| maps.tokens_on_screen().and_then(|(tokens, _)| tokens.get(name)));
    let caption = match selected {
        Some(token) => format!(
            "{}, {}x{} at ({}, {})",
            token.name(),
            token.size(),
            token.size(),
            token.x(),
            token.y()
        ),
        None => "no token selected".to_owned(),
    };
    for mut text in captions.iter_mut() {
        if text.0 != caption {
            text.0.clone_from(&caption);
        }
    }

    let disabled_wanted = selected.is_none();
    for (entity, disabled) in needing.iter() {
        if disabled == disabled_wanted {
            continue;
        }
        if disabled_wanted {
            commands.entity(entity).insert(InteractionDisabled);
        } else {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}

fn panel() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(232),
            left: px(12),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: px(8),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        CampaignChrome
        TokenPanel
        Children [
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                }
                Children [
                    (Text("Token") ThemedText),
                    field(TokenFieldKind::Name),
                    size_button(1),
                    size_button(2),
                    size_button(3),
                    size_button(4),
                    intent_button("Clear tokens", TokenIntent::Clear)
                ]
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                }
                Children [
                    (Text("no token selected") ThemedText SelectedTokenCaption),
                    field(TokenFieldKind::Rename),
                    (intent_button("Rename", TokenIntent::Rename) NeedsSelectedToken InteractionDisabled),
                    (intent_button("Delete", TokenIntent::Delete) NeedsSelectedToken InteractionDisabled)
                ]
            )
        ]
    }
}

fn field(kind: TokenFieldKind) -> impl Scene {
    bsn! {
        @FeathersTextInputContainer
        Node { width: px(120) }
        Children [
            (
                @FeathersTextInput
                TokenField { kind: {kind} }
                on(|change: On<TextEditChange>,
                    texts: Query<(&EditableText, &TokenField)>,
                    mut fields: ResMut<TokenFields>| {
                    let Ok((text, field)) = texts.get(change.event_target()) else {
                        return;
                    };
                    let value = text.value().to_string();
                    match field.kind {
                        TokenFieldKind::Name => fields.name = value,
                        TokenFieldKind::Rename => fields.rename = value,
                    }
                })
            )
        ]
    }
}

fn size_button(size: u8) -> impl Scene {
    debug_assert!((MIN_TOKEN_SIZE..=MAX_TOKEN_SIZE).contains(&size));
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({size.to_string()}) ThemedText },
        }
        TokenSizeButton { size: {size} }
        on(|activate: On<Activate>,
            buttons: Query<&TokenSizeButton>,
            mut fields: ResMut<TokenFields>,
            mut focus: ResMut<InputFocus>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            fields.size = button.size;
            focus.clear();
        })
    }
}

fn intent_button(caption: &'static str, asks: TokenIntent) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({caption.to_string()}) ThemedText },
        }
        on(move |_: On<Activate>, mut intent: ResMut<TokenIntent>, mut focus: ResMut<InputFocus>| {
            *intent = asks;
            focus.clear();
        })
    }
}

#[cfg(test)]
mod tests {
    use campaign::CombatMap;
    use campaign::feature::CellPoint;

    use super::*;

    fn app() -> App {
        let mut app = App::new();
        let mut maps = CombatMaps::default();
        maps.open(CombatMap::new("Ambush", 12, 12).unwrap(), "ambush.ron".to_owned(), None);
        app.init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<PointerOverUi>()
            .init_resource::<StatusMessage>()
            .init_resource::<TokenGesture>()
            .insert_resource(TokenFields {
                name: "orc".to_owned(),
                size: 2,
                rename: String::new(),
            })
            .insert_resource(maps)
            .insert_resource(MapPointer {
                cell: None,
                cells_per_pixel: 1.0,
            });
        app
    }

    fn point_at(app: &mut App, x: f32, y: f32) {
        app.world_mut().resource_mut::<MapPointer>().cell = Some(CellPoint::new(x, y));
    }

    fn press(app: &mut App) {
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        buttons.clear();
        buttons.press(MouseButton::Left);
        app.update();
    }

    fn release(app: &mut App) {
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        buttons.clear();
        buttons.release(MouseButton::Left);
        app.update();
    }

    fn hold(app: &mut App) {
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.update();
    }

    // A token is session state: placing one names it on the map on screen and leaves the
    // map's document clean with nothing to undo, so a close after a fight asks nothing.
    #[test]
    fn clicks_with_the_token_tool_place_numbered_tokens_and_author_nothing() {
        let mut app = app();
        app.add_systems(Update, place_tokens);
        for x in [1.2, 5.7] {
            point_at(&mut app, x, 1.0);
            press(&mut app);
            release(&mut app);
        }

        let maps = app.world().resource::<CombatMaps>();
        let (tokens, _) = maps.tokens_on_screen().unwrap();
        let placed: Vec<(&str, u32, u8)> = tokens.iter().map(|t| (t.name(), t.x(), t.size())).collect();
        assert_eq!(placed, [("orc1", 1, 2), ("orc2", 5, 2)]);
        let document = maps.on_screen().unwrap();
        assert!(!document.is_dirty());
        assert_eq!(document.undo_depth(), 0);
        assert_eq!(app.world().resource::<TokenGesture>().selected.as_deref(), Some("orc2"));
    }

    // The acceptance's drag: a token follows the pointer's cell, not its fraction, so it
    // lands on a cell boundary — and nothing moves until release.
    #[test]
    fn dragging_a_token_moves_it_whole_cells_on_release() {
        let mut app = app();
        {
            let mut maps = app.world_mut().resource_mut::<CombatMaps>();
            let (tokens, extent) = maps.tokens_on_screen_mut().unwrap();
            tokens.place("orc", 2, (5, 1), extent).unwrap();
        }
        app.add_systems(Update, move_tokens);

        point_at(&mut app, 6.5, 2.5);
        press(&mut app);
        point_at(&mut app, 9.9, 2.1);
        hold(&mut app);
        let anchor = |app: &App| {
            let (tokens, _) = app.world().resource::<CombatMaps>().tokens_on_screen().unwrap();
            let token = tokens.get("orc1").unwrap();
            (token.x(), token.y())
        };
        assert_eq!(anchor(&app), (5, 1), "a held drag has not moved the token");
        release(&mut app);

        assert_eq!(anchor(&app), (8, 1));
        let gesture = app.world().resource::<TokenGesture>();
        assert!(gesture.drag.is_none());
        assert_eq!(gesture.selected.as_deref(), Some("orc1"));
    }
}
