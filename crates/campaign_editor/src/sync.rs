//! Syncing the open campaign with its remote from inside the editor, and the panel that
//! asks for it.
//!
//! One job slot for every git process this starts — a sync, reading `origin`, setting
//! it — so no two ever touch the campaign directory at once. Authoring is paused while a
//! sync runs (see [`crate::features::authoring_is_live`]) so an edit cannot land between
//! the save and a reload that would replace it; the map keeps drawing throughout.

use bevy::ecs::system::SystemParam;
use bevy::feathers::controls::{FeathersButton, FeathersTextInput, FeathersTextInputContainer};
use bevy::feathers::theme::ThemedText;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, futures_lite::future};
use bevy::text::{EditableText, TextEdit, TextEditChange};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use campaign::repo::{self, GitError, RemoteOutcome, Repo, Synced};
use campaign::SystemGit;

use crate::document::WorldDoc;
use crate::features::dungeon;
use crate::features::prompt::Asking;
use crate::map::backdrop::{Backdrop, BackdropSource};
use crate::map::camera::MapCamera;
use crate::map::load::MapTerrain;
use crate::map::view::MapView;
use crate::notes::NoteJob;
use crate::{CampaignChrome, EditorSet, OpenCampaign, StatusMessage};

/// The field the campaign's remote is typed into.
#[derive(Component, Default, Clone)]
pub struct RemoteField;

/// The button that starts a sync.
#[derive(Component, Default, Clone)]
pub struct SyncButton;

/// The button that sets `origin` from [`SyncFields::remote`].
#[derive(Component, Default, Clone)]
pub struct SetRemoteButton;

/// The panel's root.
#[derive(Component, Default, Clone)]
pub struct SyncPanel;

enum GitWork {
    Sync(Result<Synced, GitError>),
    RemoteRead(Result<Option<String>, GitError>),
    RemoteSet(String, Result<(), GitError>),
}

/// What the last landed job left worth telling `pnp-ctl sync` and `pnp-ctl remote`.
#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub committed: Option<String>,
    pub remote: &'static str,
    pub changed: Vec<String>,
    pub reloaded: Vec<String>,
    pub message: String,
}

/// The one git job slot: a sync, a read of `origin`, or setting it.
///
/// One slot for every kind of git work this module starts, so no two git processes ever
/// touch the campaign directory at once — a sync running while `origin` is still being
/// read would race the same repository.
#[derive(Resource, Default)]
pub struct SyncJob {
    task: Option<Task<GitWork>>,
    /// What the last landed job left, for the control socket to report.
    pub last: Option<SyncReport>,
    /// `origin`'s URL, once it has been read or set.
    pub origin: Option<String>,
    /// Whether `origin` has been asked for this campaign — asked once, not once a frame.
    pub read_origin: bool,
}

impl SyncJob {
    /// Whether a git process is running. Nothing may start another while this is true.
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }

    /// A job that has already asked for `origin` — what the teardown test checks is
    /// reset, since `task` is private and out-of-module code cannot build one by hand.
    #[cfg(test)]
    pub(crate) fn assume_read_origin() -> Self {
        Self {
            read_origin: true,
            ..Self::default()
        }
    }
}

/// What has been typed into the Remote field but not yet landed.
#[derive(Resource, Debug, Default)]
pub struct SyncFields {
    pub remote: String,
}

/// Everything that syncs the open campaign with its remote.
pub struct SyncPlugin;

impl Plugin for SyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SyncJob>()
            .init_resource::<SyncFields>()
            .add_systems(
                Update,
                (
                    build_sync_panel.run_if(resource_added::<WorldDoc>),
                    read_origin.run_if(resource_exists::<OpenCampaign>),
                    commit_remote.run_if(resource_exists::<OpenCampaign>),
                    show_sync_panel.run_if(resource_exists_and_changed::<SyncJob>),
                )
                    .after(EditorSet::Authoring),
            );
    }
}

/// Whether a git job is running, without marking the resource changed.
///
/// A `ResMut` here would mark [`SyncJob`] changed every frame a job is in flight, which
/// would defeat [`show_sync_panel`]'s change-driven redraw, for the reason
/// [`crate::notes::a_note_job_is_running`] is written the same way.
pub fn a_git_job_is_running(job: Res<SyncJob>) -> bool {
    job.busy()
}

fn prepare_sync(
    job: &SyncJob,
    doc: &mut WorldDoc,
    notes: &NoteJob,
    asking: &Asking,
    status: &mut StatusMessage,
) -> bool {
    if job.busy() {
        status.say("a sync is already running");
        return false;
    }
    if asking.question.is_some() {
        status.say("answer the question on screen first");
        return false;
    }
    if notes.busy() {
        status.say("a note is being made; wait for it to finish before syncing");
        return false;
    }
    if let Err(error) = doc.save_everything() {
        status.say(error.to_string());
        return false;
    }
    true
}

/// Start a sync: save everything open, then commit, pull and push off the frame.
///
/// Returns whether it started. Refuses, saying why, while a job is already running,
/// while a question is up — the answer could change what gets saved — or while a note
/// is being made, since a sync's reload could replace the document a note job is about
/// to land its link on.
pub fn start_sync(
    job: &mut SyncJob,
    doc: &mut WorldDoc,
    campaign: &OpenCampaign,
    notes: &NoteJob,
    asking: &Asking,
    status: &mut StatusMessage,
) -> bool {
    if !prepare_sync(job, doc, notes, asking, status) {
        return false;
    }

    status.say("syncing…");
    job.last = None;
    let root = campaign.0.root().to_owned();
    job.task = Some(IoTaskPool::get().spawn(async move { GitWork::Sync(Repo::at(root).sync(&SystemGit)) }));
    true
}

/// Set `origin` to what [`SyncFields::remote`] holds.
///
/// Refuses while a job is running, once `origin` is already set — the issue's "sets
/// `origin` when there is none" — or on [`repo::remote_refusal`]. Returns whether it
/// started.
pub fn start_set_remote(
    job: &mut SyncJob,
    campaign: &OpenCampaign,
    fields: &SyncFields,
    status: &mut StatusMessage,
) -> bool {
    if job.busy() {
        status.say("a sync is already running");
        return false;
    }
    if let Some(origin) = &job.origin {
        status.say(format!("origin is already set to {origin}"));
        return false;
    }
    let url = fields.remote.trim().to_owned();
    if let Some(reason) = repo::remote_refusal(&url) {
        status.say(reason);
        return false;
    }

    status.say("setting the remote…");
    let root = campaign.0.root().to_owned();
    job.task = Some(IoTaskPool::get().spawn(async move {
        let result = Repo::at(root).set_origin(&SystemGit, &url);
        GitWork::RemoteSet(url, result)
    }));
    true
}

fn read_origin(mut job: ResMut<SyncJob>, campaign: Res<OpenCampaign>) {
    if job.read_origin || job.busy() {
        return;
    }
    job.read_origin = true;
    let root = campaign.0.root().to_owned();
    job.task = Some(IoTaskPool::get().spawn(async move { GitWork::RemoteRead(Repo::at(root).origin(&SystemGit)) }));
}

/// Everything a document switch clears, bundled as one parameter — a system takes at most
/// sixteen and [`land_git_job`] already asks for most of its own.
#[derive(SystemParam)]
pub(crate) struct AuthoringReset<'w> {
    selection: ResMut<'w, crate::features::select::Selection>,
    dragging: ResMut<'w, crate::features::select::Dragging>,
    drafting: ResMut<'w, crate::features::draw::Drafting>,
    stroking: ResMut<'w, crate::features::paint::Stroking>,
    placing: ResMut<'w, crate::features::image::Placing>,
    ruler: ResMut<'w, crate::features::ruler::Ruler>,
    pending: ResMut<'w, crate::features::panel::PendingLabel>,
    asking: ResMut<'w, Asking>,
}

impl AuthoringReset<'_> {
    /// Clear the selection, cancel a drag and an image placement, abandon a draft and a
    /// stroke, clear the measurement and the pending label, and settle any question.
    pub(crate) fn clear(&mut self) {
        self.selection.clear();
        self.dragging.cancel();
        self.drafting.abandon();
        self.stroking.abandon();
        self.placing.cancel();
        self.ruler.clear();
        self.pending.feature = None;
        self.pending.text.clear();
        self.asking.settle();
    }
}

/// Lands whichever git job is running.
///
/// Polls through `bypass_change_detection`, as
/// [`crate::notes::finish_note_job`] does, so a job in flight does not mark [`SyncJob`]
/// changed every frame; [`SyncJob::set_changed`] is called once it lands, which is what
/// wakes [`show_sync_panel`]. When a sync's reload replaces the document on screen, it
/// clears the same authoring state [`crate::features::dungeon::switch_document`] clears
/// on a document switch — a reload replaces the document just as thoroughly. While a combat
/// map is on screen the backdrop is left as it is, and is set from the reloaded document
/// when the GM goes back to the map.
pub fn land_git_job(
    mut job: ResMut<SyncJob>,
    mut doc: ResMut<WorldDoc>,
    campaign: Res<OpenCampaign>,
    mut backdrop: ResMut<Backdrop>,
    terrain: Res<MapTerrain>,
    mut reset: AuthoringReset,
    combat: Option<Res<crate::combat::CombatMaps>>,
    camera: Single<(&Transform, &Projection), With<MapCamera>>,
    mut fields: Query<&mut EditableText, With<RemoteField>>,
    mut sync_fields: ResMut<SyncFields>,
    mut status: ResMut<StatusMessage>,
) {
    let bypassed = job.bypass_change_detection();
    let Some(task) = bypassed.task.as_mut() else {
        return;
    };
    let Some(work) = block_on(future::poll_once(task)) else {
        return;
    };
    bypassed.task = None;

    match work {
        GitWork::Sync(Ok(synced)) => {
            let root = campaign.0.root().to_owned();
            let reloaded = doc.reload_from_disk(|path| synced.touches(&root, path));

            if reloaded.on_screen {
                reset.clear();
            }

            if reloaded.on_screen && !combat.is_some_and(|combat| combat.is_on_screen()) {
                let (width, height, source) = match doc.document.world().grid() {
                    Some(grid) => (grid.width(), grid.height(), BackdropSource::Grid),
                    None => (terrain.width, terrain.height, BackdropSource::Terrain),
                };
                let cell_size = backdrop.view.cell_size;
                let (transform, projection) = camera.into_inner();
                let here = dungeon::bookmark_of(transform, projection);
                backdrop.switch_to(MapView::new(width, height, cell_size), source, here);
            }

            let mut message = synced.summary();
            for reason in &reloaded.refused {
                message.push_str("; ");
                message.push_str(reason);
            }
            status.say(message.clone());
            bypassed.last = Some(SyncReport {
                committed: synced.commit.clone(),
                remote: remote_label(&synced.remote),
                changed: synced.changed.iter().map(|path| path.display().to_string()).collect(),
                reloaded: reloaded.paths.iter().map(|path| path.display().to_string()).collect(),
                message,
            });
        }
        GitWork::Sync(Err(error)) => {
            status.say(error.to_string());
            bypassed.last = Some(SyncReport {
                remote: "failed",
                message: error.to_string(),
                ..Default::default()
            });
        }
        GitWork::RemoteRead(Ok(origin)) => {
            if let Some(url) = &origin {
                sync_fields.remote.clone_from(url);
                for mut text in fields.iter_mut() {
                    text.queue_edit(TextEdit::SelectAll);
                    text.queue_edit(TextEdit::Backspace);
                    text.queue_edit(TextEdit::Insert(url.as_str().into()));
                }
            }
            job.origin = origin;
        }
        GitWork::RemoteRead(Err(error)) => warn!("{error}"),
        GitWork::RemoteSet(url, Ok(())) => {
            status.say(format!("origin is now {url}"));
            job.origin = Some(url);
        }
        GitWork::RemoteSet(_, Err(error)) => {
            warn!("{error}");
            status.say(error.to_string());
        }
    }

    job.set_changed();
}

fn remote_label(remote: &RemoteOutcome) -> &'static str {
    match remote {
        RemoteOutcome::NoRemote => "none",
        RemoteOutcome::Pushed => "pushed",
        RemoteOutcome::Conflict { .. } => "conflict",
        RemoteOutcome::Refused { .. } => "refused",
    }
}

fn commit_remote(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    typed_in: Query<(), With<RemoteField>>,
    mut job: ResMut<SyncJob>,
    campaign: Res<OpenCampaign>,
    fields: Res<SyncFields>,
    mut status: ResMut<StatusMessage>,
) {
    if !keys.just_pressed(KeyCode::Enter) && !keys.just_pressed(KeyCode::NumpadEnter) {
        return;
    }
    let Some(entity) = focus.get() else {
        return;
    };
    if !typed_in.contains(entity) {
        return;
    }
    start_set_remote(&mut job, &campaign, &fields, &mut status);
}

fn build_sync_panel(mut commands: Commands) {
    commands.spawn_scene(panel());
}

fn show_sync_panel(
    mut commands: Commands,
    job: Res<SyncJob>,
    sync_buttons: Query<(Entity, Has<InteractionDisabled>), With<SyncButton>>,
    remote_buttons: Query<(Entity, Has<InteractionDisabled>), With<SetRemoteButton>>,
) {
    toggle(&mut commands, sync_buttons.iter(), job.busy());
    toggle(&mut commands, remote_buttons.iter(), job.busy() || job.origin.is_some());
}

fn toggle(commands: &mut Commands, buttons: impl Iterator<Item = (Entity, bool)>, wanted: bool) {
    for (entity, disabled) in buttons {
        if disabled == wanted {
            continue;
        }
        if wanted {
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
            top: px(52),
            left: percent(50),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
        }
        UiTransform { translation: {Val2::new(Val::Percent(-50.0), Val::ZERO)} }
        CampaignChrome
        SyncPanel
        Children [
            sync_button(),
            (Text("Remote") ThemedText),
            (
                @FeathersTextInputContainer
                Node { width: px(220) }
                Children [
                    (
                        @FeathersTextInput
                        RemoteField
                        on(|change: On<TextEditChange>,
                            texts: Query<&EditableText>,
                            mut fields: ResMut<SyncFields>| {
                            if let Ok(text) = texts.get(change.event_target()) {
                                fields.remote = text.value().to_string();
                            }
                        })
                    )
                ]
            ),
            set_remote_button()
        ]
    }
}

fn sync_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("Sync") ThemedText },
        }
        SyncButton
        on(|_: On<Activate>,
            mut job: ResMut<SyncJob>,
            mut doc: ResMut<WorldDoc>,
            campaign: Res<OpenCampaign>,
            notes: Res<NoteJob>,
            asking: Res<Asking>,
            mut status: ResMut<StatusMessage>,
            mut focus: ResMut<InputFocus>| {
            start_sync(&mut job, &mut doc, &campaign, &notes, &asking, &mut status);
            focus.clear();
        })
    }
}

fn set_remote_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("Set remote") ThemedText },
        }
        SetRemoteButton
        on(|_: On<Activate>,
            mut job: ResMut<SyncJob>,
            campaign: Res<OpenCampaign>,
            fields: Res<SyncFields>,
            mut status: ResMut<StatusMessage>,
            mut focus: ResMut<InputFocus>| {
            start_set_remote(&mut job, &campaign, &fields, &mut status);
            focus.clear();
        })
    }
}

#[cfg(test)]
mod tests {
    use campaign::edit::Edit;
    use campaign::feature::{CellPoint, Feature, FeatureKind, Geometry};
    use campaign::{Document, World as CampaignWorld};

    use super::*;
    use crate::features::prompt::Question;

    fn dirty_doc(path: std::path::PathBuf) -> WorldDoc {
        let mut document = Document::new(CampaignWorld::default());
        let id = document.fresh_id();
        document
            .apply(Edit::Add {
                id,
                feature: Feature::plain(FeatureKind::Poi, Geometry::Point(CellPoint::new(0.0, 0.0))),
            })
            .expect("apply");
        WorldDoc::world_map(document, path)
    }

    // A sync that finds a job already running is refused, and leaves the document
    // unsaved — a second git process must never touch the campaign directory at once.
    #[test]
    fn prepare_sync_refuses_while_a_job_is_running() {
        let job = SyncJob {
            task: Some(IoTaskPool::get_or_init(Default::default).spawn(async { GitWork::RemoteRead(Ok(None)) })),
            ..SyncJob::default()
        };
        let mut doc = dirty_doc(std::path::PathBuf::from("world.ron"));
        let notes = NoteJob::default();
        let asking = Asking::default();
        let mut status = StatusMessage::default();

        let started = prepare_sync(&job, &mut doc, &notes, &asking, &mut status);

        assert!(!started);
        assert!(doc.document.is_dirty(), "an unsaved document must stay unsaved on a refusal");
    }

    // A question on screen refuses a sync for the same reason: the answer could still
    // change what gets saved.
    #[test]
    fn prepare_sync_refuses_while_a_question_is_up() {
        let job = SyncJob::default();
        let mut doc = dirty_doc(std::path::PathBuf::from("world.ron"));
        let notes = NoteJob::default();
        let mut asking = Asking::default();
        asking.raise(Question::UnsavedOnClose, None);
        let mut status = StatusMessage::default();

        let started = prepare_sync(&job, &mut doc, &notes, &asking, &mut status);

        assert!(!started);
        assert!(doc.document.is_dirty());
    }

    // A prepared sync saves before anything would be spawned — the whole point of
    // running the save first is that nothing reaches git that was not written.
    #[test]
    fn a_prepared_sync_saves_the_document() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("world.ron");
        let job = SyncJob::default();
        let mut doc = dirty_doc(path.clone());
        let notes = NoteJob::default();
        let asking = Asking::default();
        let mut status = StatusMessage::default();

        let started = prepare_sync(&job, &mut doc, &notes, &asking, &mut status);

        assert!(started);
        assert!(!doc.document.is_dirty());
        assert!(path.is_file());
    }
}
