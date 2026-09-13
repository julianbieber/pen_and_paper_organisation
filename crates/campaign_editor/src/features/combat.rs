//! Opening a blank or a stored combat map, painting it, saving it, and getting back to the
//! map.
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
use campaign::combat::{
    CombatEdit, CombatMap, DEFAULT_COMBAT_CELLS, file_name_refusal, stem_of, stored, unused_name,
};
use campaign::{brush, layout, slug};

use crate::combat::{CombatMaps, MAX_OPEN_COMBAT_MAPS};
use crate::document::{self, WorldDoc};
use crate::features::PointerOverUi;
use crate::features::dungeon::{self, DungeonIntent};
use crate::features::paint::{Stroking, advance_stroke, cell_of};
use crate::features::token::TokenGesture;
use crate::features::tool::ActiveTool;
use crate::map::backdrop::{Backdrop, BackdropSource, CameraBookmark};
use crate::map::camera::MapCamera;
use crate::map::chunks::PaintedCells;
use crate::map::load::{CombatTileset, MapTerrain, combat_tileset_refusal};
use crate::map::pointer::MapPointer;
use crate::map::view::MapView;
use crate::sync::AuthoringReset;
use crate::{CampaignChrome, OpenCampaign, StatusMessage};

/// What a new combat map is called when the name field is left empty.
pub const DEFAULT_COMBAT_NAME: &str = "Combat map";

/// The most stored combat maps the panel lists. Any stored map, listed or not, opens
/// through `pnp-ctl open-combat`.
pub const MAX_LISTED_STORED: usize = 16;

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
    /// Open the combat map stored under this file name inside `combat/`, or put it on
    /// screen when it is already open.
    OpenStored(String),
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

/// One button of the panel's list of stored combat maps that are not open.
#[derive(Component, Debug, Default, Clone, PartialEq)]
pub struct StoredRow {
    pub slot: usize,
    /// The file name inside `combat/` this row opens, or `None` while the slot is empty.
    pub file: Option<String>,
}

/// The row holding the stored combat maps, hidden while none is left that is not open.
#[derive(Component, Default, Clone)]
pub struct StoredList;

/// The *Back to map* button.
#[derive(Component, Default, Clone)]
pub struct LeaveCombatButton;

/// Hangs the *Combat maps* panel over the map, once there is a document to go back to.
pub fn build_combat_panel(mut commands: Commands) {
    commands.spawn_scene(panel());
}

/// Lists the open combat maps in the panel, marking the unsaved one and the one on screen,
/// lists the stored combat maps that are not open by name, hides the empty rows, and
/// enables *Back to map* only while a combat map is on screen.
pub fn show_combat_panel(
    mut commands: Commands,
    maps: Res<CombatMaps>,
    mut rows: Query<(&mut CombatRow, &mut Node, &Children), Without<StoredRow>>,
    mut stored_rows: Query<(&mut StoredRow, &mut Node, &Children), Without<CombatRow>>,
    mut lists: Query<&mut Node, (With<StoredList>, Without<CombatRow>, Without<StoredRow>)>,
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
        show_row(&mut node, entry.is_some());
        let caption = entry.map(|(_, caption)| caption.as_str()).unwrap_or_default();
        set_caption(children, &mut captions, caption);
    }

    let not_open: Vec<String> = maps.stored_not_open().into_iter().take(MAX_LISTED_STORED).collect();
    for mut node in lists.iter_mut() {
        show_row(&mut node, !not_open.is_empty());
    }
    for (mut row, mut node, children) in stored_rows.iter_mut() {
        let file = not_open.get(row.slot);
        if row.file.as_ref() != file {
            row.file = file.cloned();
        }
        show_row(&mut node, file.is_some());
        set_caption(children, &mut captions, file.map(|file| stem_of(file)).unwrap_or_default());
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

/// Opens a blank or a stored combat map, puts an open one on screen, or goes back to the
/// world document underneath, as [`CombatIntent`] asks.
///
/// A new map is refused, with the reason on the status line, while the combat tileset is
/// unusable or still loading, once [`MAX_OPEN_COMBAT_MAPS`] are open, on a size that is not
/// a number, when `combat/` cannot be listed to name its file, and on anything
/// [`CombatMap::new`] refuses. Its file name is taken from the name it is given, unique
/// against the open maps and the directory.
///
/// A stored map is refused first for a file name [`file_name_refusal`] refuses. One that is
/// already open is then put on screen; otherwise it is refused as a new map is for the
/// tileset and the count, and for anything [`CombatMap::load`] refuses — which leaves what
/// is on screen where it is and has the stored list read again.
pub fn switch_combat(
    mut intent: ResMut<CombatIntent>,
    fields: Res<CombatFields>,
    mut maps: ResMut<CombatMaps>,
    doc: Res<WorldDoc>,
    campaign: Res<OpenCampaign>,
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
    let root = campaign.0.root();
    let cannot_open = |maps: &CombatMaps| -> Option<String> {
        let tileset = match tileset.as_deref() {
            Some(tileset) => combat_tileset_refusal(&assets, tileset),
            None => Some("the combat tileset has not been read".to_owned()),
        };
        tileset.or_else(|| {
            maps.is_full().then(|| {
                format!("{MAX_OPEN_COMBAT_MAPS} combat maps are open; go back to one of them")
            })
        })
    };

    match asked {
        CombatIntent::Nothing => return,
        CombatIntent::New => {
            if let Some(reason) = cannot_open(&maps) {
                status.say(reason);
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
            let on_disk = match stored(root) {
                Ok(on_disk) => on_disk,
                Err(error) => {
                    status.say(format!("combat/ could not be listed: {error}"));
                    return;
                }
            };
            let file = slug::combat_map_name(&name, maps.files().chain(on_disk.iter().map(String::as_str)));
            maps.open(map, file, here);
            show_on_screen(&maps, &mut backdrop, &mut active, None);
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
            show_on_screen(&maps, &mut backdrop, &mut active, restore);
            status.say(format!("back in {name}"));
        }
        CombatIntent::OpenStored(file) => {
            if let Some(reason) = file_name_refusal(&file) {
                status.say(format!("combat/{file} {reason}"));
                return;
            }
            if let Ok(restore) = maps.switch_to_file(&file, here) {
                show_on_screen(&maps, &mut backdrop, &mut active, restore);
                let name = maps.on_screen().map(|map| map.content().name().to_owned());
                status.say(format!("back in {}", name.unwrap_or_default()));
            } else {
                if let Some(reason) = cannot_open(&maps) {
                    status.say(reason);
                    return;
                }
                let name = unused_name(stem_of(&file), maps.names());
                let map = match CombatMap::load(&layout::combat_map(root, &file), &name) {
                    Ok(map) => map,
                    Err(error) => {
                        error!("{error}");
                        status.say(error.to_string());
                        maps.forget_stored();
                        return;
                    }
                };
                maps.open(map, file, here);
                show_on_screen(&maps, &mut backdrop, &mut active, None);
                status.say(format!("opened {name}"));
            }
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
            active.leave_combat(doc.document.world().grid().is_some());
            status.say("back to the map");
        }
    }

    reset.clear();
}

/// Reads what the campaign's `combat/` directory holds into [`CombatMaps`].
///
/// A directory that cannot be read is said on the status line and remembered as empty, so
/// it is not read again every frame.
pub fn list_stored_combat_maps(
    campaign: Res<OpenCampaign>,
    mut maps: ResMut<CombatMaps>,
    mut status: ResMut<StatusMessage>,
) {
    match stored(campaign.0.root()) {
        Ok(files) => maps.set_stored(files),
        Err(error) => {
            status.say(format!("combat/ could not be listed: {error}"));
            maps.set_stored(Vec::new());
        }
    }
}

/// Whether the stored combat maps have to be listed again.
pub fn the_stored_list_is_stale(maps: Option<Res<CombatMaps>>) -> bool {
    maps.is_some_and(|maps| maps.needs_listing())
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
/// Escape, an undo and a redo each abandon a stroke in flight. Escape also cancels a token
/// drag in flight, or with none, clears the token selection. Saving writes the map on
/// screen to its file in `combat/`, whether or not it has changed, and says where or why
/// not on the status line.
pub fn combat_keys(
    keys: Res<ButtonInput<KeyCode>>,
    campaign: Option<Res<OpenCampaign>>,
    mut maps: ResMut<CombatMaps>,
    mut stroking: ResMut<Stroking>,
    mut painted: ResMut<PaintedCells>,
    mut tokens: ResMut<TokenGesture>,
    mut status: ResMut<StatusMessage>,
) {
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    if keys.just_pressed(KeyCode::Escape) {
        stroking.abandon();
        if tokens.drag.is_some() {
            tokens.cancel_drag();
        } else {
            tokens.clear();
        }
        return;
    }
    if control && keys.just_pressed(KeyCode::KeyS) {
        let Some(campaign) = campaign else {
            return;
        };
        match maps.save_on_screen(campaign.0.root()) {
            Some(Ok(file)) => status.say(format!("saved combat/{file}")),
            Some(Err(error)) => {
                error!("{error}");
                status.say(error.to_string());
            }
            None => {}
        }
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

fn show_on_screen(
    maps: &CombatMaps,
    backdrop: &mut Backdrop,
    active: &mut ActiveTool,
    restore: Option<CameraBookmark>,
) {
    let grid = maps
        .on_screen()
        .expect("a combat map was put on screen a moment ago")
        .content()
        .grid();
    let view = MapView::new(grid.width(), grid.height(), backdrop.view.cell_size);
    backdrop.switch_to(view, BackdropSource::Combat, restore);
    active.enter_combat();
}

fn show_row(node: &mut Node, shown: bool) {
    let display = if shown { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
    }
}

fn set_caption(children: &Children, captions: &mut Query<&mut Text>, caption: &str) {
    for child in children.iter() {
        if let Ok(mut text) = captions.get_mut(child)
            && text.0 != caption
        {
            text.0 = caption.to_owned();
        }
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
            ),
            (
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                    row_gap: px(4),
                    max_width: px(560),
                }
                StoredList
                Children [
                    (Text("Stored") ThemedText),
                    stored_row(0),
                    stored_row(1),
                    stored_row(2),
                    stored_row(3),
                    stored_row(4),
                    stored_row(5),
                    stored_row(6),
                    stored_row(7),
                    stored_row(8),
                    stored_row(9),
                    stored_row(10),
                    stored_row(11),
                    stored_row(12),
                    stored_row(13),
                    stored_row(14),
                    stored_row(15)
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

fn stored_row(slot: usize) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("") ThemedText },
        }
        StoredRow { slot: {slot}, file: {None} }
        on(|activate: On<Activate>,
            rows: Query<&StoredRow>,
            mut intent: ResMut<CombatIntent>,
            mut focus: ResMut<InputFocus>| {
            let Ok(row) = rows.get(activate.event_target()) else {
                return;
            };
            let Some(file) = row.file.clone() else {
                return;
            };
            *intent = CombatIntent::OpenStored(file);
            focus.clear();
        })
    }
}
