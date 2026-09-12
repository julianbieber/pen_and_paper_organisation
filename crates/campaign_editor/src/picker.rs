//! The panel that fills a dialog path field by walking the directory tree, and the
//! read in flight behind it.
//!
//! Every directory read runs on the IO task pool and lands when it is done, so a slow
//! disk never stalls the frame — the same rule the dialog's own campaign load
//! follows. The sheet is built once, as a child of the dialog, and shown by toggling
//! its display, as `features/prompt.rs` does.

use std::path::PathBuf;

use bevy::feathers::controls::{ButtonVariant, FeathersButton, FeathersScrollbar};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::input_focus::tab_navigation::{TabGroup, TabIndex};
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, futures_lite::future};
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, ControlOrientation, ScrollArea};
use campaign::browse::{self, BrowseError, Listing};

use crate::OpenCampaign;
use crate::dialog::{self, DialogFields, PathField, PathInput};

/// The directory picker behind every *Browse…* button in the dialog.
pub struct PickerPlugin;

impl Plugin for PickerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picker>().add_systems(
            Update,
            (
                (
                    land_listing.run_if(job_pending),
                    show_picker.run_if(resource_changed::<Picker>),
                    rebuild_rows.run_if(resource_changed::<Picker>),
                )
                    .chain(),
                cancel_on_escape.run_if(is_open),
                forget_on_open.run_if(resource_added::<OpenCampaign>),
            ),
        );
    }
}

/// The picker's target field, the read in flight, and the listing it landed.
///
/// One slot: opening a second directory replaces whatever read was in flight, so
/// dropping the old [`Task`] cancels it and the newest navigation always wins.
#[derive(Resource, Default)]
pub struct Picker {
    target: Option<PathField>,
    job: Option<Task<Result<Listing, BrowseError>>>,
    /// The most recent listing to land, if any.
    pub listing: Option<Listing>,
    /// Why the most recent read failed, if it did.
    pub trouble: Option<String>,
    generation: u64,
}

impl Picker {
    /// Opens the picker over `field`, starting the walk at `text` (falling back to
    /// home, then `/`, exactly as [`browse::open`] decides).
    pub fn open(&mut self, field: PathField, text: &str) {
        self.target = Some(field);
        self.listing = None;
        self.trouble = None;
        let text = text.to_owned();
        let home = std::env::var_os("HOME");
        self.job = Some(IoTaskPool::get().spawn(async move { browse::open(&text, home.as_deref()) }));
    }

    /// Navigates to `dir`, replacing any read already in flight.
    pub fn go(&mut self, dir: PathBuf) {
        self.job = Some(IoTaskPool::get().spawn(async move { browse::list(&dir) }));
    }

    /// Closes the picker and forgets what it was showing.
    pub fn close(&mut self) {
        self.target = None;
        self.job = None;
        self.listing = None;
        self.trouble = None;
    }

    /// Whether the picker is on screen.
    pub fn is_open(&self) -> bool {
        self.target.is_some()
    }

    /// Whether a directory read is in flight.
    pub fn reading(&self) -> bool {
        self.job.is_some()
    }
}

/// The modal sheet, built once and shown by toggling its display.
#[derive(Component, Default, Clone)]
pub struct PickerRoot;

/// The current directory's path.
#[derive(Component, Default, Clone)]
pub struct PickerPath;

/// What is reading, wrong, or left unlisted.
#[derive(Component, Default, Clone)]
pub struct PickerNote;

/// The scrolling container the picker's rows are spawned into.
#[derive(Component, Default, Clone)]
pub struct PickerRows;

/// The button that goes to the parent of the current directory.
#[derive(Component, Default, Clone)]
pub struct PickerUp;

/// The button that fills the target field with the current directory.
#[derive(Component, Default, Clone)]
pub struct PickerChoose;

/// One row of the listing, and the directory a click on it goes to.
#[derive(Component, Debug, Default, Clone)]
pub struct PickerRow {
    pub path: PathBuf,
}

fn job_pending(picker: Res<Picker>) -> bool {
    picker.job.is_some()
}

fn is_open(picker: Res<Picker>) -> bool {
    picker.is_open()
}

fn land_listing(mut picker: ResMut<Picker>) {
    let Some(task) = picker.job.as_mut() else {
        return;
    };
    let Some(result) = block_on(future::poll_once(task)) else {
        return;
    };
    picker.job = None;
    match result {
        Ok(listing) => {
            picker.listing = Some(listing);
            picker.trouble = None;
        }
        Err(error) => {
            picker.listing = Some(Listing {
                dir: error.path().to_owned(),
                subdirectories: Vec::new(),
                unlisted: 0,
            });
            picker.trouble = Some(error.to_string());
        }
    }
    picker.generation += 1;
}

fn show_picker(
    mut commands: Commands,
    picker: Res<Picker>,
    mut roots: Query<&mut Node, With<PickerRoot>>,
    mut paths: Query<&mut Text, (With<PickerPath>, Without<PickerNote>)>,
    mut notes: Query<&mut Text, (With<PickerNote>, Without<PickerPath>)>,
    mut indices: Query<&mut TabIndex>,
    ups: Query<(Entity, Has<InteractionDisabled>), With<PickerUp>>,
    chooses: Query<(Entity, Has<InteractionDisabled>), With<PickerChoose>>,
) {
    let display = if picker.is_open() { Display::Flex } else { Display::None };
    for mut node in roots.iter_mut() {
        if node.display != display {
            node.display = display;
        }
    }

    let path_text = picker
        .listing
        .as_ref()
        .map(|listing| listing.dir.display().to_string())
        .unwrap_or_default();
    for mut text in paths.iter_mut() {
        if text.0 != path_text {
            text.0 = path_text.clone();
        }
    }

    let note_text = note_text(&picker);
    for mut text in notes.iter_mut() {
        if text.0 != note_text {
            text.0 = note_text.clone();
        }
    }

    let can_go_up = picker
        .listing
        .as_ref()
        .is_some_and(|listing| browse::up(&listing.dir).is_some());
    for (entity, disabled) in ups.iter() {
        dialog::set_pressable(&mut commands, &mut indices, entity, disabled, can_go_up);
    }

    let can_choose = picker.listing.is_some() && !picker.reading();
    for (entity, disabled) in chooses.iter() {
        dialog::set_pressable(&mut commands, &mut indices, entity, disabled, can_choose);
    }
}

fn note_text(picker: &Picker) -> String {
    if let Some(trouble) = &picker.trouble {
        return trouble.clone();
    }
    if picker.reading() {
        return "reading…".to_owned();
    }
    match picker.listing.as_ref().map(|listing| listing.unlisted) {
        Some(unlisted) if unlisted > 0 => format!("{unlisted} more not shown"),
        _ => String::new(),
    }
}

fn rebuild_rows(
    mut commands: Commands,
    picker: Res<Picker>,
    rows: Query<Entity, With<PickerRows>>,
    mut shown: Local<u64>,
) {
    if picker.generation == *shown {
        return;
    }
    *shown = picker.generation;

    for rows_entity in rows.iter() {
        commands.entity(rows_entity).despawn_children();
        let Some(listing) = picker.listing.as_ref() else {
            continue;
        };
        let scenes: Vec<_> = listing
            .subdirectories
            .iter()
            .map(|sub| picker_row(sub.name.clone(), sub.path.clone()))
            .collect();
        commands
            .entity(rows_entity)
            .queue_spawn_related_scenes::<Children>(scenes)
            .insert(ScrollPosition::default());
    }
}

fn cancel_on_escape(keys: Res<ButtonInput<KeyCode>>, mut picker: ResMut<Picker>) {
    if keys.just_pressed(KeyCode::Escape) {
        picker.close();
    }
}

fn forget_on_open(mut picker: ResMut<Picker>) {
    picker.close();
}

/// Builds the modal sheet, hidden, as a child of the dialog.
pub fn sheet() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(10),
            padding: px(20),
        }
        GlobalZIndex(50)
        ThemeBackgroundColor(tokens::WINDOW_BG)
        TabGroup { order: 0, modal: true }
        PickerRoot
        Children [
            (Text("Choose a directory") ThemedText),
            (Text("") ThemedText PickerPath),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(8),
                }
                Children [ up_button() ]
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    width: px(480),
                    height: px(320),
                }
                Children [
                    (
                        #rows
                        Node {
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Stretch,
                            height: percent(100),
                            overflow: Overflow::scroll_y(),
                        }
                        ScrollArea
                        PickerRows
                    ),
                    (
                        @FeathersScrollbar {
                            @target: #rows,
                            @orientation: {ControlOrientation::Vertical},
                        }
                        Node {
                            position_type: PositionType::Absolute,
                            right: px(0),
                            top: px(0),
                            bottom: px(0),
                            width: px(6),
                        }
                    )
                ]
            ),
            (Text("") ThemedText PickerNote),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(8),
                }
                Children [ choose_button(), cancel_button() ]
            )
        ]
    }
}

fn up_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("Up") ThemedText },
        }
        PickerUp
        InteractionDisabled
        TabIndex(-1)
        ThemeBackgroundColor(tokens::WINDOW_BG)
        on(|_: On<Activate>, mut picker: ResMut<Picker>| {
            let Some(up) = picker.listing.as_ref().and_then(|listing| browse::up(&listing.dir)) else {
                return;
            };
            picker.go(up);
        })
    }
}

fn choose_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("Choose") ThemedText },
            @variant: ButtonVariant::Primary,
        }
        PickerChoose
        InteractionDisabled
        TabIndex(-1)
        on(|_: On<Activate>,
            mut picker: ResMut<Picker>,
            mut fields: ResMut<DialogFields>,
            mut inputs: Query<(&PathInput, &mut EditableText)>| {
            if picker.reading() {
                return;
            }
            let Some(target) = picker.target else {
                return;
            };
            let Some(dir) = picker.listing.as_ref().map(|listing| listing.dir.clone()) else {
                return;
            };
            let text = dir.display().to_string();
            fields.set_path(target, text.clone());
            for (input, mut field) in inputs.iter_mut() {
                if input.0 == target {
                    dialog::replace_text(&mut field, &text);
                }
            }
            picker.close();
        })
    }
}

fn cancel_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("Cancel") ThemedText },
        }
        on(|_: On<Activate>, mut picker: ResMut<Picker>| {
            picker.close();
        })
    }
}

fn picker_row(name: String, path: PathBuf) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({name}) ThemedText },
        }
        PickerRow { path: {path} }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        on(|activate: On<Activate>, rows: Query<&PickerRow>, mut picker: ResMut<Picker>| {
            let Ok(row) = rows.get(activate.event_target()) else {
                return;
            };
            picker.go(row.path.clone());
        })
    }
}
