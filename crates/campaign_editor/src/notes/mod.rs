//! Making notes from the editor: the one note in flight, whether `zk` is there at all,
//! and the rule saying which note buttons can be pressed.
//!
//! Every route that starts a note — the property panel's Create button, the notes panel's
//! three, the control socket's verb — goes through [`start`], for the reason
//! [`doc::apply`](crate::features::doc::apply) exists: a refusal that says nothing is
//! indistinguishable from a press that was never noticed, and three callers deciding
//! separately when a note may be made is three answers to one question.
//!
//! There is one job slot and no queue. A second Create while one is running is refused
//! rather than held, because two notes for one feature would leave the second unreachable
//! — only the last `SetNote` would survive.
//!
//! Whether `zk` is installed is asked once a session and kept in [`ZkState`], so a button
//! can say notes are unavailable *before* it is pressed. Discovering it after the press
//! would satisfy nobody: the criterion is that the buttons explain themselves.

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, futures_lite::future};
use campaign::edit::Edit;
use campaign::feature::FeatureId;
use campaign::notebook::{NewNote, NoteError, NoteKind, Notebook, SystemRunner, zk_is_installed};

use crate::features::doc::{self, WorldDoc};
use crate::features::select::Selection;
use crate::{OpenCampaign, StatusMessage};

/// The panel that creates the notes no feature holds.
pub mod panel;

/// What the note being made is for.
///
/// Carried on the job rather than read from the [`Selection`] when it lands, because the
/// GM may have selected something else in the meantime — and a
/// [`FeatureId`] is never reused, so an id that has gone stays gone rather than naming
/// somebody else's feature.
#[derive(Resource, Default)]
pub struct NoteJob {
    task: Option<Task<Result<NewNote, NoteError>>>,
    subject: Option<FeatureId>,
}

impl NoteJob {
    /// Whether a note is being made. Nothing may start another while this is true.
    ///
    /// Derived from the task rather than stored beside it: a flag and a task that
    /// disagreed would either poll a job that is gone or leave every note button disabled
    /// for the life of the process.
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
}

/// Whether `zk` answers, asked once.
///
/// `answered` is what the buttons read; until it is true the honest state is "not known
/// yet", which is not the same as available, so nothing is pressable.
#[derive(Resource, Default)]
pub struct ZkState {
    probe: Option<Task<bool>>,
    /// Whether the question has been settled at all.
    pub answered: bool,
    /// Whether `zk` is there, meaningful only once `answered`.
    pub present: bool,
}

impl ZkState {
    /// Why notes cannot be made right now, or `None` when they can.
    pub fn refusal(&self) -> Option<&'static str> {
        match (self.answered, self.present) {
            (false, _) => Some("looking for zk…"),
            (true, false) => Some("zk is not installed, so notes are unavailable"),
            (true, true) => None,
        }
    }
}

/// What has been typed into the notes panel's title field.
///
/// A resource rather than read off the field at the press, the way the open dialog keeps
/// its paths: a note title never lands on the undo stack, so unlike a feature's label
/// there is nothing to hold it back for.
#[derive(Resource, Default)]
pub struct NoteTitle(pub String);

/// Everything that makes a note.
pub struct NotesPlugin;

impl Plugin for NotesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NoteJob>()
            .init_resource::<ZkState>()
            .init_resource::<NoteTitle>()
            .add_systems(Update, probe_zk.run_if(zk_has_not_answered));
    }
}

fn zk_has_not_answered(state: Res<ZkState>) -> bool {
    !state.answered
}

/// Whether a note job is running, without marking the resource changed.
///
/// A `ResMut` here would mark [`NoteJob`] changed every frame a job is in flight, which
/// would both defeat the change-driven panel updates and make "changed" useless as the
/// signal that a note landed.
pub fn a_note_job_is_running(job: Res<NoteJob>) -> bool {
    job.busy()
}

/// Asks once whether `zk` is installed, and lands the answer.
///
/// One system rather than two, and the started-ness is the slot being full rather than a
/// second flag beside it — a flag and a task that disagreed would start a probe every
/// frame for the life of the process.
///
/// The probe holds its own slot rather than [`NoteJob`]'s, or the first Create of a
/// session would queue behind it and count against the one note at a time.
pub fn probe_zk(mut state: ResMut<ZkState>) {
    let Some(task) = state.probe.as_mut() else {
        state.probe = Some(IoTaskPool::get().spawn(async move { zk_is_installed() }));
        return;
    };
    let Some(present) = block_on(future::poll_once(task)) else {
        return;
    };
    state.probe = None;
    state.answered = true;
    state.present = present;
    if !present {
        warn!("zk is not installed; the map works, notes do not");
    }
}

/// Start making a note of `kind` titled `title`, for `subject` when it has one.
///
/// The one place that decides whether a note may be started, and the only place that
/// spawns the work. Returns whether it started, and says why not on `status` when it did
/// not — a press that lands on nothing is indistinguishable from one that was never
/// delivered.
pub fn start(
    job: &mut NoteJob,
    zk: &ZkState,
    campaign: &OpenCampaign,
    status: &mut StatusMessage,
    kind: NoteKind,
    title: &str,
    subject: Option<FeatureId>,
) -> bool {
    if let Some(reason) = refusal(job, zk, title) {
        status.say(reason);
        return false;
    }

    let notebook = Notebook::of(campaign.0.root());
    let title = title.trim().to_owned();
    status.say(format!("making a {} note for {title}…", kind.label()));

    job.subject = subject;
    job.task = Some(IoTaskPool::get().spawn(async move {
        notebook.create(&SystemRunner, kind, &title, subject)
    }));
    true
}

/// Why a note cannot be started right now, or `None` when one can.
///
/// Shared by [`start`] and by the systems that disable the buttons, so what a button says
/// and what a press does cannot come apart.
pub fn refusal(job: &NoteJob, zk: &ZkState, title: &str) -> Option<String> {
    if let Some(reason) = zk.refusal() {
        return Some(reason.to_owned());
    }
    if job.busy() {
        return Some("a note is already being made".to_owned());
    }
    if title.trim().is_empty() {
        return Some("a note needs a title".to_owned());
    }
    None
}

/// Open `path` in the GM's editor, saying why if it cannot be.
///
/// Never takes the job slot: `zk edit` runs an editor, which does not return until the
/// note is closed, so it is started and forgotten.
pub fn open(campaign: &OpenCampaign, status: &mut StatusMessage, path: &str) {
    let notebook = Notebook::of(campaign.0.root());
    match notebook.open(&SystemRunner, path) {
        Ok(()) => status.say(format!("opened {path}")),
        Err(error) => {
            warn!("{error}");
            status.say(error.to_string());
        }
    }
}

/// Lands the note `zk` made, onto the feature that asked for it.
///
/// Runs ahead of everything that reads the document, so a landed note reaches the panel,
/// the selection and the map on the frame it arrived rather than the frame after.
///
/// The created path is said on every branch, including the one that succeeds: undo takes
/// the link back but not the file, so this message is the only record the GM has of where
/// the note went.
pub fn finish_note_job(
    mut job: ResMut<NoteJob>,
    mut doc: ResMut<WorldDoc>,
    mut status: ResMut<StatusMessage>,
) {
    let job = job.bypass_change_detection();
    let Some(task) = job.task.as_mut() else {
        return;
    };
    let Some(result) = block_on(future::poll_once(task)) else {
        return;
    };
    let subject = job.subject.take();
    job.task = None;

    match result {
        Ok(note) => match subject {
            Some(id) => {
                let edit = Edit::SetNote {
                    id,
                    note: Some(note.path.clone()),
                };
                if doc::apply(&mut doc, &mut status, edit) {
                    status.say(format!("made {}", note.path));
                } else {
                    status.say(format!(
                        "made {}, but it could not be linked: {id} is gone",
                        note.path
                    ));
                }
            }
            None => status.say(format!("made {}", note.path)),
        },
        Err(error) => {
            warn!("{error}");
            status.say(error.to_string());
        }
    }
}

/// Marks the job changed once it has emptied, so the buttons come back.
///
/// Split from [`finish_note_job`] because that one polls through
/// `bypass_change_detection` — the point of which is that a job in flight does not mark
/// the resource every frame. Something still has to say when it landed, and this is it.
pub fn note_job_settled(mut job: ResMut<NoteJob>, mut was_busy: Local<bool>) {
    let busy = job.busy();
    if busy != *was_busy {
        *was_busy = busy;
        job.set_changed();
    }
}

/// The feature the selection holds, when it holds exactly one.
pub fn selected(doc: &WorldDoc, selection: &Selection) -> Option<(FeatureId, campaign::Feature)> {
    let id = selection.only()?;
    let feature = doc.document.world().feature(id)?;
    Some((id, feature.clone()))
}
