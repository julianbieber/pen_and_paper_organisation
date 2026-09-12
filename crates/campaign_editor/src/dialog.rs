//! What the editor shows when it has no campaign: the two paths it needs to open or
//! create one, and whatever went wrong last time.
//!
//! Opening a campaign loads its terrain, which is as slow as the terrain is large, so
//! the work happens off the render thread and lands when it is done. Opening reads the
//! manifest on the spot first, because a mistyped path costs a few hundred bytes to
//! reject and answering in the same frame is worth more than answering uniformly.
//! Creating has no manifest to read yet, so it goes straight to the task pool.

use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};
use bevy::text::{EditableText, TextEditChange};
use bevy::ui_widgets::Activate;
use campaign::{Campaign, CampaignError, CampaignManifest};

use crate::{OpenCampaign, StatusMessage};

/// The dialog and everything behind it: the typed paths, the load in flight, and the
/// systems that land it. Its systems run only while no campaign is open.
pub struct DialogPlugin;

impl Plugin for DialogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DialogFields>()
            .init_resource::<OpenJob>()
            .add_systems(
                Update,
                finish_open.run_if(not(resource_exists::<OpenCampaign>)),
            );
    }
}

/// The paths typed into the dialog.
#[derive(Resource, Default)]
pub struct DialogFields {
    /// The campaign directory to open, or to create.
    pub root: String,
    /// The terrain a created campaign copies in. Ignored when opening, which takes
    /// the terrain from the manifest.
    pub terrain: String,
}

/// The campaign load in flight, if there is one.
///
/// One slot, deliberately: a second load started over the first would be a second
/// multi-hundred-megabyte read for a result nothing would use.
#[derive(Resource, Default)]
pub struct OpenJob(Option<Task<Result<Campaign, CampaignError>>>);

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

/// The dialog's scene: two paths and two actions.
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
                                on(|change: On<TextEditChange>,
                                    texts: Query<&EditableText>,
                                    mut fields: ResMut<DialogFields>| {
                                    if let Ok(text) = texts.get(change.event_target()) {
                                        fields.root = text.value().to_string();
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
                    (Text("Terrain") ThemedText),
                    (
                        @FeathersTextInputContainer
                        Node { width: px(320) }
                        Children [
                            (
                                @FeathersTextInput
                                TerrainInput
                                on(|change: On<TextEditChange>,
                                    texts: Query<&EditableText>,
                                    mut fields: ResMut<DialogFields>| {
                                    if let Ok(text) = texts.get(change.event_target()) {
                                        fields.terrain = text.value().to_string();
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
                            start_open(&fields, &mut job, &mut status);
                        })
                    ),
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

fn start_open(fields: &DialogFields, job: &mut OpenJob, status: &mut StatusMessage) {
    if job.busy() {
        return;
    }
    let root = std::path::PathBuf::from(fields.root.trim());

    if let Err(error) = CampaignManifest::read(&root) {
        status.0 = error.to_string();
        return;
    }

    status.0 = format!("opening {}…", root.display());
    let task = AsyncComputeTaskPool::get().spawn(async move { Campaign::open(root) });
    job.0 = Some(task);
}

fn start_create(fields: &DialogFields, job: &mut OpenJob, status: &mut StatusMessage) {
    if job.busy() {
        return;
    }
    let root = std::path::PathBuf::from(fields.root.trim());
    let terrain = std::path::PathBuf::from(fields.terrain.trim());

    status.0 = format!("creating {}…", root.display());
    let task = AsyncComputeTaskPool::get().spawn(async move { Campaign::create(root, terrain) });
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
        Ok(campaign) => {
            info!("opened campaign at {}", campaign.root().display());
            status.opened(&campaign);
            commands.insert_resource(OpenCampaign(campaign));
        }
        Err(error) => {
            error!("{error}");
            status.0 = error.to_string();
        }
    }
}
