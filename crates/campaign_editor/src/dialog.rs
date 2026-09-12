//! What the editor shows when it has no campaign: what it needs to open or create
//! one, whatever went wrong last time, and the campaigns opened before.
//!
//! Opening a campaign loads its terrain, which is as slow as the terrain is large, so
//! the work happens off the render thread and lands when it is done. Opening reads the
//! manifest on the spot first, because a mistyped path costs a few hundred bytes to
//! reject and answering in the same frame is worth more than answering uniformly.
//! Creating has no manifest to read yet, so it goes straight to the task pool.
//!
//! It also remembers every campaign that opens, however it was opened, so the dialog
//! has something to list next time.

use std::path::PathBuf;

use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool, Task, block_on, futures_lite::future};
use bevy::text::{EditableText, TextEdit, TextEditChange};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use campaign::recent::{self, Listed, MAX_RECENT, RecentError};
use campaign::{Campaign, CampaignError, CampaignManifest, Created, SystemGit};

use crate::picker::Picker;
use crate::{OpenCampaign, StatusMessage};

/// The dialog and everything behind it: the typed paths, the load in flight, the
/// recent-campaigns scan, and the systems that land them all. Most of its systems run
/// only while no campaign is open; [`remember_campaign`] is the one that runs on the
/// frame one opens.
pub struct DialogPlugin;

impl Plugin for DialogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DialogFields>()
            .init_resource::<OpenJob>()
            .init_resource::<RecentList>()
            .add_systems(
                Update,
                (
                    finish_open.run_if(not(resource_exists::<OpenCampaign>)),
                    scan_recents.run_if(recents_have_not_answered),
                    show_recents.run_if(resource_changed::<RecentList>),
                    remember_campaign.run_if(resource_added::<OpenCampaign>),
                    seed_parent
                        .run_if(resource_changed::<RecentList>)
                        .run_if(not(resource_exists::<OpenCampaign>)),
                    fill_new_inputs.run_if(not(resource_exists::<OpenCampaign>)),
                ),
            );
    }
}

/// One of the dialog's three path fields, for anything that must address one — a
/// `Browse…` button, or the input it fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathField {
    #[default]
    Root,
    Parent,
    Terrain,
}

/// Marks a text input as holding one of the dialog's path fields, so a picker can
/// find it and replace its text.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct PathInput(pub PathField);

/// A `Browse…` button beside a path field, naming the field it fills.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct BrowseButton(pub PathField);

/// The fields typed into the dialog.
#[derive(Resource, Default)]
pub struct DialogFields {
    /// The campaign directory to open.
    pub root: String,
    /// The terrain a created campaign copies in. Ignored when opening, which takes
    /// the terrain from the manifest.
    pub terrain: String,
    /// What a created campaign is called; its directory is derived from it.
    pub name: String,
    /// The directory a created campaign goes in.
    pub parent: String,
}

impl DialogFields {
    /// The text currently held by `field`.
    pub fn path(&self, field: PathField) -> &str {
        match field {
            PathField::Root => &self.root,
            PathField::Parent => &self.parent,
            PathField::Terrain => &self.terrain,
        }
    }

    /// Replaces the text held by `field`.
    pub fn set_path(&mut self, field: PathField, text: String) {
        match field {
            PathField::Root => self.root = text,
            PathField::Parent => self.parent = text,
            PathField::Terrain => self.terrain = text,
        }
    }
}

/// The campaign load in flight, if there is one.
///
/// One slot, deliberately: a second load started over the first would be a second
/// multi-hundred-megabyte read for a result nothing would use.
#[derive(Resource, Default)]
pub struct OpenJob(Option<Task<Result<Created, CampaignError>>>);

impl OpenJob {
    /// Whether a load is running. While one is, the buttons are disabled.
    pub fn busy(&self) -> bool {
        self.0.is_some()
    }
}

#[derive(Component, Default, Clone)]
struct RootInput;

#[derive(Component, Default, Clone)]
struct TerrainInput;

#[derive(Component, Default, Clone)]
struct NameInput;

#[derive(Component, Default, Clone)]
struct ParentInput;

/// The recent-campaigns scan in flight, and what it found.
///
/// One slot, in [`crate::notes::probe_zk`]'s shape: the slot being full is the
/// started-ness, so there is no second flag to keep in step with it.
#[derive(Resource, Default)]
pub struct RecentList {
    job: Option<Task<Result<Vec<Listed>, RecentError>>>,
    /// Whether the scan has settled, one way or another.
    pub answered: bool,
    /// The remembered campaigns, in the order they were remembered.
    pub listed: Vec<Listed>,
    /// Why the scan came back with nothing to show, when it did.
    pub trouble: Option<String>,
}

/// The heading above the recent-campaigns rows.
#[derive(Component, Default, Clone)]
pub struct RecentsHeading;

/// One row of the recent-campaigns list, and the entry it is currently showing.
///
/// The root is carried here rather than looked up by `slot` when the row is pressed,
/// for the reason [`references::ReferenceRow`](crate::notes::references::ReferenceRow)
/// carries its path: a press must open what the GM is looking at.
#[derive(Component, Debug, Default, Clone)]
pub struct RecentRow {
    pub slot: usize,
    pub root: Option<PathBuf>,
    pub present: bool,
}

/// The dialog's scene: an Open section and a Create section.
///
/// The status line is deliberately not part of it: the map reports a failure there
/// long after the dialog has been closed.
pub fn dialog() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(10),
            padding: px(20),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            (Text("Open a campaign") ThemedText),
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column,
                       width: px(420), row_gap: px(4) }
                Children [
                    (Text("") ThemedText RecentsHeading),
                    recent_row(0), recent_row(1), recent_row(2), recent_row(3),
                    recent_row(4), recent_row(5), recent_row(6), recent_row(7)
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
                    (Text("Campaign") ThemedText),
                    (
                        @FeathersTextInputContainer
                        Node { width: px(320) }
                        Children [
                            (
                                @FeathersTextInput
                                RootInput
                                PathInput(PathField::Root)
                                on(|change: On<TextEditChange>,
                                    texts: Query<&EditableText>,
                                    mut fields: ResMut<DialogFields>| {
                                    if let Ok(text) = texts.get(change.event_target()) {
                                        fields.root = text.value().to_string();
                                    }
                                })
                            )
                        ]
                    ),
                    browse_button(PathField::Root)
                ]
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(8),
                }
                Children [
                    (
                        @FeathersButton {
                            @caption: bsn! { Text("Open") ThemedText },
                            @variant: ButtonVariant::Primary,
                        }
                        on(|_: On<Activate>,
                            fields: Res<DialogFields>,
                            mut job: ResMut<OpenJob>,
                            mut status: ResMut<StatusMessage>| {
                            start_open(PathBuf::from(fields.root.trim()), &mut job, &mut status);
                        })
                    )
                ]
            ),
            (Text("Create a campaign") ThemedText),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                }
                Children [
                    (Text("Name") ThemedText),
                    (
                        @FeathersTextInputContainer
                        Node { width: px(320) }
                        Children [
                            (
                                @FeathersTextInput
                                NameInput
                                on(|change: On<TextEditChange>,
                                    texts: Query<&EditableText>,
                                    mut fields: ResMut<DialogFields>| {
                                    if let Ok(text) = texts.get(change.event_target()) {
                                        fields.name = text.value().to_string();
                                    }
                                })
                            )
                        ]
                    )
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
                    (Text("Where") ThemedText),
                    (
                        @FeathersTextInputContainer
                        Node { width: px(320) }
                        Children [
                            (
                                @FeathersTextInput
                                ParentInput
                                PathInput(PathField::Parent)
                                on(|change: On<TextEditChange>,
                                    texts: Query<&EditableText>,
                                    mut fields: ResMut<DialogFields>| {
                                    if let Ok(text) = texts.get(change.event_target()) {
                                        fields.parent = text.value().to_string();
                                    }
                                })
                            )
                        ]
                    ),
                    browse_button(PathField::Parent)
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
                    (Text("Terrain") ThemedText),
                    (
                        @FeathersTextInputContainer
                        Node { width: px(320) }
                        Children [
                            (
                                @FeathersTextInput
                                TerrainInput
                                PathInput(PathField::Terrain)
                                on(|change: On<TextEditChange>,
                                    texts: Query<&EditableText>,
                                    mut fields: ResMut<DialogFields>| {
                                    if let Ok(text) = texts.get(change.event_target()) {
                                        fields.terrain = text.value().to_string();
                                    }
                                })
                            )
                        ]
                    ),
                    browse_button(PathField::Terrain)
                ]
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(8),
                }
                Children [
                    (
                        @FeathersButton {
                            @caption: bsn! { Text("Create") ThemedText },
                        }
                        on(|_: On<Activate>,
                            fields: Res<DialogFields>,
                            mut job: ResMut<OpenJob>,
                            mut status: ResMut<StatusMessage>| {
                            start_create(&fields, &mut job, &mut status);
                        })
                    )
                ]
            )
        ]
    }
}

fn browse_button(field: PathField) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("Browse…") ThemedText },
        }
        BrowseButton({field})
        on(|activate: On<Activate>,
            buttons: Query<&BrowseButton>,
            fields: Res<DialogFields>,
            mut picker: ResMut<Picker>,
            mut focus: ResMut<InputFocus>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            picker.open(button.0, fields.path(button.0));
            focus.clear();
        })
    }
}

/// Starts opening the campaign at `root`, off the render thread. Does nothing if `job`
/// is already busy. Reads the manifest on the spot first, so a mistyped path is answered
/// on `status` in the same frame rather than after the async load fails.
pub(crate) fn start_open(root: PathBuf, job: &mut OpenJob, status: &mut StatusMessage) {
    if job.busy() {
        return;
    }

    if let Err(error) = CampaignManifest::read(&root) {
        status.0 = error.to_string();
        return;
    }

    status.0 = format!("opening {}…", root.display());
    let task = AsyncComputeTaskPool::get().spawn(async move {
        Campaign::open(root).map(|campaign| Created {
            campaign,
            repository: Ok(()),
        })
    });
    job.0 = Some(task);
}

fn start_create(fields: &DialogFields, job: &mut OpenJob, status: &mut StatusMessage) {
    if job.busy() {
        return;
    }
    let name = fields.name.trim().to_owned();
    let parent = std::path::PathBuf::from(fields.parent.trim());
    let terrain = std::path::PathBuf::from(fields.terrain.trim());

    let root = match campaign::campaign::root_for(&parent, &name) {
        Ok(root) => root,
        Err(error) => {
            status.0 = error.to_string();
            return;
        }
    };

    status.0 = format!("creating {}…", root.display());
    let task = AsyncComputeTaskPool::get()
        .spawn(async move { Campaign::create_named(parent, &name, terrain, &SystemGit) });
    job.0 = Some(task);
}

fn finish_open(
    mut commands: Commands,
    mut job: ResMut<OpenJob>,
    mut status: ResMut<StatusMessage>,
) {
    let Some(task) = job.0.as_mut() else {
        return;
    };
    let Some(result) = block_on(future::poll_once(task)) else {
        return;
    };
    job.0 = None;

    match result {
        Ok(created) => {
            info!("opened campaign at {}", created.campaign.root().display());
            status.created(&created);
            commands.insert_resource(OpenCampaign(created.campaign));
        }
        Err(error) => {
            error!("{error}");
            status.0 = error.to_string();
        }
    }
}

const _: () = assert!(MAX_RECENT == 8, "the dialog scene writes out MAX_RECENT rows by hand");

fn recents_have_not_answered(list: Res<RecentList>) -> bool {
    !list.answered
}

fn scan_recents(mut list: ResMut<RecentList>, mut status: ResMut<StatusMessage>) {
    let Some(task) = list.job.as_mut() else {
        let Some(path) = recent::file() else {
            list.answered = true;
            list.trouble = Some(
                "there is no config directory, so campaigns are not remembered".to_owned(),
            );
            return;
        };
        list.job = Some(IoTaskPool::get().spawn(async move { recent::survey(&path) }));
        return;
    };
    let Some(result) = block_on(future::poll_once(task)) else {
        return;
    };
    list.job = None;
    list.answered = true;

    match result {
        Ok(listed) => list.listed = listed,
        Err(error) => {
            warn!("{error}");
            status.say(error.to_string());
            list.trouble = Some(error.to_string());
        }
    }
}

fn show_recents(
    mut commands: Commands,
    list: Res<RecentList>,
    mut rows: Query<(Entity, &mut RecentRow, &Children, Has<InteractionDisabled>)>,
    mut headings: Query<&mut Text, With<RecentsHeading>>,
    mut captions: Query<&mut Text, Without<RecentsHeading>>,
    mut indices: Query<&mut TabIndex>,
) {
    let heading_text = recents_heading(&list);
    for mut text in headings.iter_mut() {
        if text.0 != heading_text {
            text.0 = heading_text.clone();
        }
    }

    for (entity, mut row, children, disabled) in rows.iter_mut() {
        let entry = list.listed.get(row.slot);
        let root = entry.map(|listed| listed.recent.root.clone());
        let present = entry.is_some_and(|listed| listed.present);
        if row.root != root {
            row.root = root;
        }
        if row.present != present {
            row.present = present;
        }
        for child in children.iter() {
            if let Ok(mut text) = captions.get_mut(child) {
                let shown = entry.map(recent_caption).unwrap_or_default();
                if text.0 != shown {
                    text.0 = shown;
                }
            }
        }
        set_pressable(&mut commands, &mut indices, entity, disabled, entry.is_some());
    }
}

fn recents_heading(list: &RecentList) -> String {
    if let Some(trouble) = &list.trouble {
        return trouble.clone();
    }
    if !list.answered {
        return "looking for recent campaigns…".to_owned();
    }
    if list.listed.is_empty() {
        return "no campaigns opened yet".to_owned();
    }
    "Recent campaigns".to_owned()
}

fn recent_caption(listed: &Listed) -> String {
    let Listed { recent, present } = listed;
    if *present {
        format!("{} — {}", recent.name, recent.root.display())
    } else {
        format!("{} — {} (missing)", recent.name, recent.root.display())
    }
}

/// Replaces the whole content of a text input: select all, delete, insert `text`.
///
/// Shared by [`seed_parent`] and [`crate::picker`]'s Choose button — both fill an
/// input's text from outside a keystroke, and both need the edit queued the same way.
pub(crate) fn replace_text(field: &mut EditableText, text: &str) {
    field.queue_edit(TextEdit::SelectAll);
    field.queue_edit(TextEdit::Backspace);
    field.queue_edit(TextEdit::Insert(text.into()));
}

/// Enables `entity` for input when `pressable`, disables it otherwise: toggles
/// [`InteractionDisabled`] and sets its [`TabIndex`] to `0` or `-1` to match. Writes
/// only where the current state differs from `pressable`; an entity with no
/// `TabIndex` still gets its `InteractionDisabled` state toggled.
pub(crate) fn set_pressable(
    commands: &mut Commands,
    indices: &mut Query<&mut TabIndex>,
    entity: Entity,
    disabled: bool,
    pressable: bool,
) {
    if disabled == pressable {
        if pressable {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    if let Ok(mut index) = indices.get_mut(entity) {
        let wanted = if pressable { 0 } else { -1 };
        if index.0 != wanted {
            index.0 = wanted;
        }
    }
}

fn remember_campaign(campaign: Res<OpenCampaign>) {
    let Some(path) = recent::file() else {
        return;
    };
    let root = campaign.0.root().to_owned();
    let name = campaign.0.manifest().name.clone();
    IoTaskPool::get()
        .spawn(async move {
            if let Err(error) = recent::record(&path, &root, &name) {
                warn!("{error}");
            }
        })
        .detach();
}

fn seed_parent(
    list: Res<RecentList>,
    mut fields: ResMut<DialogFields>,
    mut inputs: Query<&mut EditableText, With<ParentInput>>,
    mut seeded: Local<bool>,
) {
    if *seeded || !list.answered {
        return;
    }
    *seeded = true;
    if !fields.parent.is_empty() {
        return;
    }

    let newest = list.listed.first().map(|listed| listed.recent.root.as_path());
    let home = std::env::var_os("HOME");
    let Some(parent) = recent::default_parent(newest, home.as_deref()) else {
        return;
    };

    let text = parent.display().to_string();
    fields.parent = text.clone();
    for mut field in inputs.iter_mut() {
        replace_text(&mut field, &text);
    }
}

fn fill_new_inputs(
    fields: Res<DialogFields>,
    mut paths: Query<(&PathInput, &mut EditableText), (Added<PathInput>, Without<NameInput>)>,
    mut names: Query<&mut EditableText, (Added<NameInput>, Without<PathInput>)>,
) {
    for (path, mut field) in paths.iter_mut() {
        let text = fields.path(path.0);
        if !text.is_empty() {
            replace_text(&mut field, text);
        }
    }
    for mut field in names.iter_mut() {
        if !fields.name.is_empty() {
            replace_text(&mut field, &fields.name);
        }
    }
}

fn recent_row(slot: usize) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("") ThemedText },
        }
        RecentRow { slot: {slot}, root: {None}, present: {false} }
        InteractionDisabled
        TabIndex(-1)
        ThemeBackgroundColor(tokens::WINDOW_BG)
        on(|activate: On<Activate>,
            rows: Query<&RecentRow>,
            mut job: ResMut<OpenJob>,
            mut status: ResMut<StatusMessage>| {
            let Ok(row) = rows.get(activate.event_target()) else {
                return;
            };
            let Some(root) = row.root.clone() else {
                return;
            };
            if !row.present {
                status.say(format!(
                    "`{}` no longer holds a {}",
                    root.display(),
                    campaign::layout::MANIFEST_FILE
                ));
                return;
            }
            start_open(root, &mut job, &mut status);
        })
    }
}
