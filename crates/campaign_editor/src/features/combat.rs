//! Opening a blank combat map, painting it, and getting back to the map.
//!
//! The buttons are read the way [`crate::features::dungeon`] reads its own: an observer
//! writes what was pressed into [`CombatIntent`] or [`CombatFields`] and a system consumes
//! it. No system reads a button.

use bevy::feathers::controls::{FeathersButton, FeathersTextInput, FeathersTextInputContainer};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use campaign::brush;
use campaign::combat::{CombatEdit, CombatMap, DEFAULT_COMBAT_CELLS, unused_name};

use crate::combat::{CombatMaps, MAX_OPEN_COMBAT_MAPS};
use crate::document::{self, WorldDoc};
use crate::features::PointerOverUi;
use crate::features::dungeon::{self, DungeonIntent};
use crate::features::paint::{Stroking, advance_stroke, cell_of};
use crate::features::tool::ActiveTool;
use crate::map::backdrop::{Backdrop, BackdropSource};
use crate::map::camera::MapCamera;
use crate::map::chunks::PaintedCells;
use crate::map::load::{CombatTileset, MapTerrain, combat_tileset_refusal};
use crate::map::pointer::MapPointer;
use crate::map::view::MapView;
use crate::sync::AuthoringReset;
use crate::{CampaignChrome, StatusMessage};

/// What a new combat map is called when the name field is left empty.
pub const DEFAULT_COMBAT_NAME: &str = "Combat map";

/// What the GM asked for, written by a button's observer and consumed by
/// [`switch_combat`].
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub enum CombatIntent {
    #[default]
    Nothing,
    /// Open a blank combat map from [`CombatFields`].
    New,
    /// Put the open combat map with this name on screen.
    Open(String),
    /// Go back to the world document underneath.
    Leave,
}

/// Whether anything was asked for.
pub fn a_combat_switch_was_asked_for(intent: Res<CombatIntent>) -> bool {
    *intent != CombatIntent::Nothing
}

/// What has been typed into the *New combat map* form.
///
/// An empty field means the default: [`DEFAULT_COMBAT_NAME`] for the name and
/// [`DEFAULT_COMBAT_CELLS`] for either side, so one press with nothing typed opens a map.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct CombatFields {
    pub name: String,
    pub width: String,
    pub height: String,
}

/// Which of [`CombatFields`] a text input writes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    #[default]
    Name,
    Width,
    Height,
}

/// A text input in the *New combat map* form.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub struct CombatField {
    pub kind: FieldKind,
}

/// The *Combat maps* panel's root.
#[derive(Component, Default, Clone)]
pub struct CombatPanel;

/// One row of the panel's list of open combat maps.
#[derive(Component, Debug, Default, Clone, PartialEq)]
pub struct CombatRow {
    pub slot: usize,
    /// The combat map this row opens, or `None` while the slot is empty.
    pub name: Option<String>,
}

/// The *Back to map* button.
#[derive(Component, Default, Clone)]
pub struct LeaveCombatButton;

/// Hangs the *Combat maps* panel over the map, once there is a document to go back to.
pub fn build_combat_panel(mut commands: Commands) {
    commands.spawn_scene(panel());
}

/// Lists the open combat maps in the panel, marking the unsaved one and the one on screen,
/// hides the empty rows, and enables *Back to map* only while a combat map is on screen.
pub fn show_combat_panel(
    mut commands: Commands,
    maps: Res<CombatMaps>,
    mut rows: Query<(&mut CombatRow, &mut Node, &Children)>,
    mut captions: Query<&mut Text>,
    leave: Query<(Entity, Has<InteractionDisabled>), With<LeaveCombatButton>>,
) {
    let listed: Vec<(String, String)> = maps
        .listed()
        .map(|(document, on_screen)| {
            let name = document.content().name().to_owned();
            let mut caption = name.clone();
            if document.is_dirty() {
                caption.push_str(" *");
            }
            if on_screen {
                caption.push_str(" (open)");
            }
            (name, caption)
        })
        .collect();

    for (mut row, mut node, children) in rows.iter_mut() {
        let entry = listed.get(row.slot);
        let name = entry.map(|(name, _)| name.clone());
        if row.name != name {
            row.name = name;
        }
        let display = if entry.is_some() { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let caption = entry.map(|(_, caption)| caption.as_str()).unwrap_or_default();
        for child in children.iter() {
            if let Ok(mut text) = captions.get_mut(child)
                && text.0 != caption
            {
                text.0 = caption.to_owned();
            }
        }
    }

    let disabled_wanted = !maps.is_on_screen();
    for (entity, disabled) in leave.iter() {
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

/// Opens a blank combat map, puts an open one on screen, or goes back to the world
/// document underneath, as [`CombatIntent`] asks.
///
/// A new map is refused, with the reason on the status line, while the combat tileset is
/// unusable or still loading, once [`MAX_OPEN_COMBAT_MAPS`] are open, on a size that is not
/// a number, and on anything [`CombatMap::new`] refuses.
pub fn switch_combat(
    mut intent: ResMut<CombatIntent>,
    fields: Res<CombatFields>,
    mut maps: ResMut<CombatMaps>,
    doc: Res<WorldDoc>,
    terrain: Res<MapTerrain>,
    assets: Res<AssetServer>,
    tileset: Option<Res<CombatTileset>>,
    mut backdrop: ResMut<Backdrop>,
    mut reset: AuthoringReset,
    mut active: ResMut<ActiveTool>,
    mut status: ResMut<StatusMessage>,
    camera: Single<(&Transform, &Projection), With<MapCamera>>,
) {
    let asked = std::mem::take(&mut *intent);
    let (transform, projection) = camera.into_inner();
    let here = dungeon::bookmark_of(transform, projection);
    let cell_size = backdrop.view.cell_size;

    match asked {
        CombatIntent::Nothing => return,
        CombatIntent::New => {
            let refusal = match tileset.as_deref() {
                Some(tileset) => combat_tileset_refusal(&assets, tileset),
                None => Some("the combat tileset has not been read".to_owned()),
            };
            if let Some(reason) = refusal {
                status.say(reason);
                return;
            }
            if maps.is_full() {
                status.say(format!(
                    "{MAX_OPEN_COMBAT_MAPS} combat maps are open; go back to one of them"
                ));
                return;
            }
            let (width, height) = match (cells_in(&fields.width), cells_in(&fields.height)) {
                (Ok(width), Ok(height)) => (width, height),
                (Err(reason), _) | (_, Err(reason)) => {
                    status.say(reason);
                    return;
                }
            };
            let typed = fields.name.trim();
            let wanted = if typed.is_empty() { DEFAULT_COMBAT_NAME } else { typed };
            let name = unused_name(wanted, maps.names());
            let map = match CombatMap::new(&name, width, height) {
                Ok(map) => map,
                Err(problem) => {
                    status.say(problem.to_string());
                    return;
                }
            };
            maps.open(map, here);
            backdrop.switch_to(MapView::new(width, height, cell_size), BackdropSource::Combat, None);
            active.enter_combat();
            status.say(format!("opened {name}"));
        }
        CombatIntent::Open(name) => {
            let restore = match maps.switch_to(&name, here) {
                Ok(restore) => restore,
                Err(reason) => {
                    status.say(reason);
                    return;
                }
            };
            let grid = maps
                .on_screen()
                .expect("a combat map was put on screen a moment ago")
                .content()
                .grid();
            let (width, height) = (grid.width(), grid.height());
            backdrop.switch_to(MapView::new(width, height, cell_size), BackdropSource::Combat, restore);
            active.enter_combat();
            status.say(format!("back in {name}"));
        }
        CombatIntent::Leave => {
            let Some(restore) = maps.leave(here) else {
                status.say("no combat map is open");
                return;
            };
            match doc.document.world().grid() {
                Some(grid) => backdrop.switch_to(
                    MapView::new(grid.width(), grid.height(), cell_size),
                    BackdropSource::Grid,
                    restore,
                ),
                None => backdrop.switch_to(
                    MapView::new(terrain.width, terrain.height, cell_size),
                    BackdropSource::Terrain,
                    restore,
                ),
            }
            active.leave_a_grid_if(doc.document.world().grid().is_none());
            status.say("back to the map");
        }
    }

    reset.clear();
}

/// Refuses a dungeon switch asked for while a combat map is on screen, before
/// [`dungeon::switch_document`] could act on the world document underneath.
pub fn refuse_dungeon_switch(mut intent: ResMut<DungeonIntent>, mut status: ResMut<StatusMessage>) {
    *intent = DungeonIntent::Nothing;
    status.say("go back to the map first");
}

/// Turns the left button on the combat map on screen into one paint edit per stroke.
pub fn paint_combat_tiles(
    buttons: Res<ButtonInput<MouseButton>>,
    pointer: Res<MapPointer>,
    over_ui: Res<PointerOverUi>,
    active: Res<ActiveTool>,
    mut stroking: ResMut<Stroking>,
    mut maps: ResMut<CombatMaps>,
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
    let Some(map) = maps.on_screen_mut() else {
        return;
    };
    let changes = brush::cells(map.content().grid(), active.brush, &path, active.combat_tile);
    if changes.is_empty() {
        return;
    }

    let count = changes.len();
    let cells: Vec<(u32, u32)> = changes.iter().map(|change| (change.x, change.y)).collect();
    match map.apply(CombatEdit::PaintTiles { changes }) {
        Ok(()) => {
            painted.cells.extend(cells);
            status.say(format!("painted {count} cell(s)"));
        }
        Err(refusal) => document::report(&mut status, &refusal),
    }
}

/// Undo, redo, save and escape on the combat map on screen.
///
/// Escape, an undo and a redo each abandon a stroke in flight. Saving is not offered yet,
/// and says so on the status line.
pub fn combat_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut maps: ResMut<CombatMaps>,
    mut stroking: ResMut<Stroking>,
    mut painted: ResMut<PaintedCells>,
    mut status: ResMut<StatusMessage>,
) {
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    if keys.just_pressed(KeyCode::Escape) {
        stroking.abandon();
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyS) {
        status.say("a combat map cannot be saved yet");
        return;
    }
    if !(control && keys.just_pressed(KeyCode::KeyZ)) {
        return;
    }

    stroking.abandon();
    let Some(map) = maps.on_screen_mut() else {
        return;
    };
    let stepped = if shift { map.redo() } else { map.undo() };
    match stepped {
        Ok(true) => painted.everything = true,
        Ok(false) => status.say(if shift {
            "nothing to redo"
        } else {
            "nothing to undo"
        }),
        Err(refusal) => document::report(&mut status, &refusal),
    }
}

fn cells_in(typed: &str) -> Result<u32, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Ok(DEFAULT_COMBAT_CELLS);
    }
    typed
        .parse()
        .map_err(|_| format!("{typed} is not a number of cells"))
}

fn panel() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(96),
            left: percent(50),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: px(8),
        }
        UiTransform { translation: {Val2::new(Val::Percent(-50.0), Val::ZERO)} }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        CampaignChrome
        CombatPanel
        Children [
            (Text("Combat maps") ThemedText),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                }
                Children [
                    (Text({format!("Name ({DEFAULT_COMBAT_NAME})")}) ThemedText),
                    field(FieldKind::Name, 140.0),
                    (Text({format!("W ({DEFAULT_COMBAT_CELLS})")}) ThemedText),
                    field(FieldKind::Width, 48.0),
                    (Text({format!("H ({DEFAULT_COMBAT_CELLS})")}) ThemedText),
                    field(FieldKind::Height, 48.0),
                    new_button(),
                    leave_button()
                ]
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(6),
                    row_gap: px(4),
                    max_width: px(560),
                }
                Children [
                    combat_row(0),
                    combat_row(1),
                    combat_row(2),
                    combat_row(3),
                    combat_row(4),
                    combat_row(5),
                    combat_row(6),
                    combat_row(7)
                ]
            )
        ]
    }
}

fn field(kind: FieldKind, width: f32) -> impl Scene {
    bsn! {
        @FeathersTextInputContainer
        Node { width: {Val::Px(width)} }
        Children [
            (
                @FeathersTextInput
                CombatField { kind: {kind} }
                on(|change: On<TextEditChange>,
                    texts: Query<(&EditableText, &CombatField)>,
                    mut fields: ResMut<CombatFields>| {
                    let Ok((text, field)) = texts.get(change.event_target()) else {
                        return;
                    };
                    let value = text.value().to_string();
                    match field.kind {
                        FieldKind::Name => fields.name = value,
                        FieldKind::Width => fields.width = value,
                        FieldKind::Height => fields.height = value,
                    }
                })
            )
        ]
    }
}

fn new_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("New combat map") ThemedText },
        }
        on(|_: On<Activate>, mut intent: ResMut<CombatIntent>, mut focus: ResMut<InputFocus>| {
            *intent = CombatIntent::New;
            focus.clear();
        })
    }
}

fn leave_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("Back to map") ThemedText },
        }
        LeaveCombatButton
        InteractionDisabled
        on(|_: On<Activate>, mut intent: ResMut<CombatIntent>, mut focus: ResMut<InputFocus>| {
            *intent = CombatIntent::Leave;
            focus.clear();
        })
    }
}

fn combat_row(slot: usize) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("") ThemedText },
        }
        CombatRow { slot: {slot}, name: {None} }
        on(|activate: On<Activate>,
            rows: Query<&CombatRow>,
            mut intent: ResMut<CombatIntent>,
            mut focus: ResMut<InputFocus>| {
            let Ok(row) = rows.get(activate.event_target()) else {
                return;
            };
            let Some(name) = row.name.clone() else {
                return;
            };
            *intent = CombatIntent::Open(name);
            focus.clear();
        })
    }
}
