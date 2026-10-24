//! How the GM handles the tokens on the combat map on screen, and how those tokens look.
//!
//! A token is never saved, but every placement, move, rename and removal of one is a
//! [`CombatEdit`] on its map's undo stack, so a misclick is one press of undo. The tokens
//! themselves live in their map's document in [`CombatMaps`], which is what keeps them while
//! the map is parked; only what the GM has typed, chosen and is holding lives here, and the
//! holding is cleared wherever every other gesture is.
//!
//! What a token is called, where it lands, what it covers and which one a cell picks are all
//! asked of [`campaign::token`], and the edit for each change of [`campaign::CombatMap`].
//! This module owns the clicks, the panel and the pixels.

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
use campaign::{CombatEdit, CombatMap, Document};

use crate::combat::CombatMaps;
use crate::document;
use crate::features::PointerOverUi;
use crate::features::paint::cell_of;
use crate::features::pens::{
    TOKEN_LABEL_PIXELS, TOKEN_LABEL_SHARE, TOKEN_LABEL_WIDTH_SHARE, TOKEN_LETTER_ASPECT, TokenPens,
};
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
/// sized from [`TokenFields`], as one edit, and selects it.
///
/// A press over the UI places nothing. A refusal — a press whose token would cover another
/// among them — is said on the status line.
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
    let (Some(cell), Some(map)) = (pointer.cell, maps.on_screen()) else {
        return;
    };
    let edit = match map.content().place_token_edit(&fields.name, fields.size, cell_of(cell)) {
        Ok(edit) => edit,
        Err(problem) => {
            status.say(problem.to_string());
            return;
        }
    };
    let CombatEdit::PlaceToken { token, .. } = &edit else {
        unreachable!("a place edit places a token");
    };
    let name = token.name().to_owned();
    if apply_token_edit(&mut maps, edit, &mut status) {
        status.say(format!("placed {name}"));
        gesture.drag = None;
        gesture.selected = Some(name);
    }
}

/// Selects the topmost token under a press on the combat map on screen and carries a drag
/// of it until release, when it lands on whole cells as one edit.
///
/// A press on no token selects none. A press over the UI does nothing, but a release over
/// it still lands the drag. A drag that would land on another token leaves it where it was
/// and says which token is in the way.
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
    let Some(map) = maps.on_screen() else {
        return;
    };
    let edit = match map.content().move_token_edit(&drag.name, drag.grabbed, drag.latest) {
        Ok(Some(edit)) => edit,
        Ok(None) => return,
        Err(problem) => {
            status.say(problem.to_string());
            return;
        }
    };
    let CombatEdit::MoveToken { to, .. } = edit else {
        unreachable!("a move edit moves a token");
    };
    if apply_token_edit(&mut maps, edit, &mut status) {
        status.say(format!("moved {} to ({}, {})", drag.name, to.0, to.1));
    }
}

/// Renames, deletes or clears as [`TokenIntent`] asks, on the combat map on screen, as one
/// edit each, and says the outcome on the status line.
///
/// A rename or a delete with no token selected, a clear of a map carrying none, or any
/// intent with no combat map on screen, is said and does nothing. A rename to the name the
/// token already has does nothing.
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
    let Some(map) = maps.on_screen().map(Document::content) else {
        status.say("no combat map is on screen");
        return;
    };
    if asked == TokenIntent::Clear {
        let count = map.tokens().len();
        let Some(edit) = map.clear_tokens_edit() else {
            status.say("there are no tokens to clear");
            return;
        };
        if apply_token_edit(&mut maps, edit, &mut status) {
            gesture.clear();
            status.say(format!("cleared {count} token(s)"));
        }
        return;
    }
    let Some(selected) = gesture.selected.clone() else {
        status.say("select a token first");
        return;
    };
    let edit = match asked {
        TokenIntent::Rename => map.rename_token_edit(&selected, &fields.rename),
        TokenIntent::Delete => map.remove_token_edit(&selected).map(Some),
        TokenIntent::Nothing | TokenIntent::Clear => return,
    };
    let edit = match edit {
        Ok(Some(edit)) => edit,
        Ok(None) => return,
        Err(problem) => {
            status.say(problem.to_string());
            return;
        }
    };
    let renamed = match &edit {
        CombatEdit::RenameToken { to, .. } => Some(to.clone()),
        _ => None,
    };
    if !apply_token_edit(&mut maps, edit, &mut status) {
        return;
    }
    match renamed {
        Some(name) => {
            status.say(format!("renamed {selected} to {name}"));
            gesture.selected = Some(name);
        }
        None => {
            status.say(format!("deleted {selected}"));
            gesture.clear();
        }
    }
}

fn apply_token_edit(maps: &mut CombatMaps, edit: CombatEdit, status: &mut StatusMessage) -> bool {
    let Some(map) = maps.on_screen_mut() else {
        return false;
    };
    match map.apply(edit) {
        Ok(()) => true,
        Err(refusal) => {
            document::report(status, &refusal);
            false
        }
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
/// would land — in red when it would land on another token and so stay where it is.
pub fn draw_tokens(
    backdrop: Res<Backdrop>,
    maps: Res<CombatMaps>,
    gesture: Res<TokenGesture>,
    camera: Single<&Projection, With<MapCamera>>,
    mut pens: TokenPens,
) {
    let Some(map) = maps.on_screen().map(Document::content) else {
        return;
    };
    let (tokens, extent) = (map.tokens(), map.extent());
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
        let letters = token.name().chars().count().max(1) as f32;
        let lettering = lettering
            .min(side * TOKEN_LABEL_SHARE)
            .min(side * TOKEN_LABEL_WIDTH_SHARE / (letters * TOKEN_LETTER_ASPECT));
        let centre = view.cell_corner_to_world(x as f32, y as f32) + Vec2::new(side / 2.0, -side / 2.0);
        let selected = gesture.selected.as_deref() == Some(token.name());
        let blocked = dragged.is_some() && lands_on_another(map, gesture.drag.as_ref());
        let colour = if blocked {
            Color::from(css::RED)
        } else if selected {
            Color::from(css::GOLD)
        } else {
            Color::WHITE
        };

        pens.ring.rect_2d(centre, Vec2::splat(side), colour.with_alpha(0.5));
        pens.ring.circle_2d(centre, side * 0.42, colour);
        let at = Isometry3d::from_translation(centre.extend(TOKEN_Z));
        pens.shadow.text(at, token.name(), lettering, Vec2::ZERO, Color::BLACK.with_alpha(0.8));
        pens.label.text(at, token.name(), lettering, Vec2::ZERO, Color::WHITE);
    }
}

fn lands_on_another(map: &CombatMap, drag: Option<&TokenDrag>) -> bool {
    drag.is_some_and(|drag| map.move_token_edit(&drag.name, drag.grabbed, drag.latest).is_err())
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
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
            }
            Children [
                Text("Token") ThemedText
                --
                @field(TokenFieldKind::Name)
                --
                @size_button(1)
                --
                @size_button(2)
                --
                @size_button(3)
                --
                @size_button(4)
                --
                @intent_button("Clear tokens", TokenIntent::Clear)
            ]
            --
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
            }
            Children [
                Text("no token selected") ThemedText SelectedTokenCaption
                --
                @field(TokenFieldKind::Rename)
                --
                @intent_button("Rename", TokenIntent::Rename) NeedsSelectedToken InteractionDisabled
                --
                @intent_button("Delete", TokenIntent::Delete) NeedsSelectedToken InteractionDisabled
            ]
        ]
    }
}

fn field(kind: TokenFieldKind) -> impl Scene {
    bsn! {
        @FeathersTextInputContainer
        Node { width: px(120) }
        Children [
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

    // A token is never saved: placing one names it on the map on screen and is one step of
    // undo each, but leaves the map's document clean, so a close after a fight asks nothing.
    #[test]
    fn clicks_with_the_token_tool_place_numbered_tokens_on_the_history_only() {
        let mut app = app();
        app.add_systems(Update, place_tokens);
        for x in [1.2, 4.8] {
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
        assert_eq!(document.undo_depth(), 2);
        assert_eq!(app.world().resource::<TokenGesture>().selected.as_deref(), Some("orc2"));
    }

    // The mistake seen at the table: a second click on a token with the token tool placed
    // another of the same kind under it. It now places nothing and says what is there.
    #[test]
    fn a_click_with_the_token_tool_on_a_token_places_nothing() {
        let mut app = app();
        app.add_systems(Update, place_tokens);
        for x in [1.2, 1.6] {
            point_at(&mut app, x, 1.0);
            press(&mut app);
            release(&mut app);
        }

        let maps = app.world().resource::<CombatMaps>();
        assert_eq!(maps.tokens_on_screen().unwrap().0.len(), 1);
        assert_eq!(maps.on_screen().unwrap().undo_depth(), 1);
        assert_eq!(app.world().resource::<StatusMessage>().0, "orc1 is already there");
    }

    // The acceptance's drag: a token follows the pointer's cell, not its fraction, so it
    // lands on a cell boundary — and nothing moves until release.
    #[test]
    fn dragging_a_token_moves_it_whole_cells_on_release() {
        let mut app = app();
        {
            let mut maps = app.world_mut().resource_mut::<CombatMaps>();
            let map = maps.on_screen_mut().unwrap();
            let place = map.content().place_token_edit("orc", 2, (5, 1)).unwrap();
            map.apply(place).unwrap();
        }
        app.add_systems(Update, move_tokens);

        point_at(&mut app, 6.2, 2.4);
        press(&mut app);
        point_at(&mut app, 9.4, 2.1);
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
