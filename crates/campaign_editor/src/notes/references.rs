//! What references the selected place: the tag the selection asks about, the one query in
//! flight, the answers already had, and the rows that show them.
//!
//! The query holds its own slot rather than the one [`NoteJob`](crate::notes::NoteJob)
//! uses, for the reason the `zk` probe does: a note the GM asked for must never queue
//! behind a query nothing asked for. Nothing here can refuse a note, and
//! [`refusal`](crate::notes::refusal) never learns this slot exists.
//!
//! Two rules bound the subprocesses: a tag is asked about only once it has been the
//! wanted one for [`QUERY_SETTLE`], and only one query runs at a time.
//!
//! An answer is cached under the tag it was *asked for*, never the tag wanted when it
//! lands, so a selection that moved on stores the answer rather than mis-filing it. An
//! answer asked of a notebook that has since changed is discarded instead: it describes a
//! state the watcher already invalidated.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bevy::feathers::controls::FeathersButton;
use bevy::feathers::theme::{ThemedText, ThemeBackgroundColor};
use bevy::feathers::tokens;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, futures_lite::future};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use campaign::feature::FeatureId;
use campaign::notebook::{NoteError, NoteKind, Notebook, Reference, SystemRunner, tag_of};

use crate::document::WorldDoc;
use crate::features::select::Selection;
use crate::notes::ZkState;
use crate::{OpenCampaign, StatusMessage};

/// How many references the panel has room for. A hard cap, not a page: what is past it
/// is counted in the heading and reachable only through a note that is shown.
pub const REFERENCE_ROWS: usize = 8;

/// How long a tag must be the wanted one before it is worth a subprocess.
pub const QUERY_SETTLE: Duration = Duration::from_millis(200);

/// What came back for one tag.
#[derive(Debug, Clone)]
pub enum Answer {
    /// The notes carrying the tag, newest first, without the subject's own.
    Found(Vec<Reference>),
    /// `zk` was asked and refused. Cached so a broken notebook is not re-asked every
    /// frame, and carried as a message so the heading can say what happened rather than
    /// showing an empty list that reads as "nothing references this".
    Failed(String),
}

/// What the panel should show, decided once so no two branches can disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View<'a> {
    Unavailable,
    NothingSelected,
    NoNoteYet,
    Looking,
    Failed(&'a str),
    Found(&'a [Reference]),
}

struct InFlight {
    tag: String,
    generation: u64,
    task: Task<Result<Vec<Reference>, NoteError>>,
}

/// The selected feature's references: which tag, which answers, and the query in flight.
#[derive(Resource, Default)]
pub struct References {
    /// The one selected feature, when exactly one is selected.
    ///
    /// Kept even when that feature has no note, because "nothing is selected" and "this
    /// has no note yet" are different things to say and only this tells them apart.
    /// Written by [`follow_selection`] alone, and only where the value differs — a write
    /// per frame of a drag would re-run every reader for the length of the gesture.
    pub subject: Option<FeatureId>,
    /// The tag the subject is found by, or `None` when it has no note. Written by
    /// [`follow_selection`] alone, under the same rule as `subject`.
    pub tag: Option<String>,
    cached: HashMap<String, Answer>,
    generation: u64,
    query: Option<InFlight>,
    wanted_since: Option<Instant>,
}

impl References {
    /// Whether a query is running.
    pub fn busy(&self) -> bool {
        self.query.is_some()
    }

    /// The answer already had for the wanted tag, if any.
    pub fn answer(&self) -> Option<&Answer> {
        self.cached.get(self.tag.as_deref()?)
    }

    /// What the panel should show, given whether `zk` is there at all.
    ///
    /// One ordered decision rather than a branch per widget: written as independent
    /// conditions, "nothing is selected" and "no answer yet" both hold at once and the
    /// panel would settle on whichever was written last.
    pub fn view<'a>(&'a self, zk: &ZkState) -> View<'a> {
        if zk.refusal().is_some() {
            return View::Unavailable;
        }
        if self.subject.is_none() {
            return View::NothingSelected;
        }
        if self.tag.is_none() {
            return View::NoNoteYet;
        }
        match self.answer() {
            None => View::Looking,
            Some(Answer::Failed(why)) => View::Failed(why),
            Some(Answer::Found(found)) => View::Found(found),
        }
    }

    fn want(&mut self, subject: Option<FeatureId>, tag: Option<String>) {
        if self.subject == subject && self.tag == tag {
            return;
        }
        if self.tag != tag {
            self.wanted_since = Some(Instant::now());
        }
        self.subject = subject;
        self.tag = tag;
    }

    /// Forget every answer, and mark whatever is in flight as describing a notebook that
    /// no longer exists.
    pub fn invalidate(&mut self) {
        self.cached.clear();
        self.generation = self.generation.wrapping_add(1);
    }

    /// Forget the wanted tag's answer, so it is asked again.
    pub fn refresh(&mut self) {
        if let Some(tag) = self.tag.clone() {
            self.cached.remove(&tag);
        }
        self.generation = self.generation.wrapping_add(1);
        self.wanted_since = Some(Instant::now() - QUERY_SETTLE);
    }

    fn worth_asking(&self) -> bool {
        let Some(tag) = self.tag.as_deref() else {
            return false;
        };
        if self.cached.contains_key(tag) {
            return false;
        }
        self.wanted_since
            .is_some_and(|since| since.elapsed() >= QUERY_SETTLE)
    }
}

/// Whether the query system has anything to do, without marking the resource changed.
pub fn a_reference_query_has_work(references: Res<References>, zk: Res<ZkState>) -> bool {
    references.busy() || (zk.present && references.worth_asking())
}

/// Works out which tag the selection asks about.
///
/// A feature's note is always a place note — the Create button is the only thing that
/// makes one, and it always makes a place — so the kind is not read back out of the
/// note's frontmatter. Nothing in this tool parses a note.
pub fn follow_selection(
    doc: Res<WorldDoc>,
    selection: Res<Selection>,
    mut references: ResMut<References>,
) {
    let subject = selection.only();
    let tag = subject
        .and_then(|id| doc.document.world().feature(id))
        .and_then(|feature| feature.note.as_deref())
        .map(|note| tag_of(NoteKind::of_a_feature(), note));

    let inner = references.bypass_change_detection();
    let before = (inner.subject, inner.tag.clone());
    inner.want(subject, tag);
    let moved = before != (inner.subject, inner.tag.clone());
    if moved {
        references.set_changed();
    }
}

/// Keeps at most one `zk list` in flight, and lands its answer in the cache.
///
/// Polls through `bypass_change_detection` for the reason
/// [`finish_note_job`](crate::notes::finish_note_job) does: a `ResMut` taken every frame
/// a query is in flight marks the resource every frame, which would make
/// [`show_references`]'s run condition true for ever and re-decide eight rows at frame
/// rate. Only a landed answer marks it.
///
/// Says nothing on the status line. Creating a note trips the watcher and so triggers a
/// query within a frame or two, and a refusal here would overwrite the "made `<path>`"
/// message that is the GM's only record of where that note went.
pub fn run_reference_query(
    campaign: Res<OpenCampaign>,
    zk: Res<ZkState>,
    mut references: ResMut<References>,
) {
    if landed(references.bypass_change_detection()) {
        references.set_changed();
        return;
    }

    let references = references.bypass_change_detection();
    if references.query.is_some() || !zk.present || !references.worth_asking() {
        return;
    }

    let tag = references.tag.clone().expect("worth_asking checked the tag");
    let subject_slug = slug_in(&tag).to_owned();
    let notebook = Notebook::of(campaign.0.root());
    let asked = tag.clone();

    references.query = Some(InFlight {
        tag,
        generation: references.generation,
        task: IoTaskPool::get().spawn(async move {
            notebook.references(&SystemRunner, &asked, &subject_slug)
        }),
    });
}

fn landed(references: &mut References) -> bool {
    if references.query.is_none() {
        return false;
    }
    let result = references
        .query
        .as_mut()
        .and_then(|query| block_on(future::poll_once(&mut query.task)));
    let Some(result) = result else {
        return false;
    };
    let InFlight { tag, generation, .. } =
        references.query.take().expect("it was there a line ago");

    if generation != references.generation {
        return false;
    }
    let answer = match result {
        Ok(found) => Answer::Found(found),
        Err(error) => {
            warn!("{error}");
            Answer::Failed(error.to_string())
        }
    };
    references.cached.insert(tag, answer);
    true
}

fn slug_in(tag: &str) -> &str {
    tag.rsplit('/').next().unwrap_or(tag)
}

/// The panel's references section: a heading, [`REFERENCE_ROWS`] rows and Refresh.
pub fn section() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
        }
        Children [
            (Text("") ThemedText ReferencesHeading),
            reference_row(0), reference_row(1), reference_row(2), reference_row(3),
            reference_row(4), reference_row(5), reference_row(6), reference_row(7),
            refresh_button()
        ]
    }
}

/// The heading above the reference rows.
#[derive(Component, Default, Clone)]
pub struct ReferencesHeading;

/// One row of the references list, and the note it is currently showing.
///
/// The path is carried here rather than looked up by `slot` when the row is pressed: the
/// cache can be dropped between the frame that filled the row and the press, and a press
/// must open what the GM is looking at.
#[derive(Component, Debug, Default, Clone)]
pub struct ReferenceRow {
    pub slot: usize,
    pub path: Option<String>,
}

/// The button that asks the wanted tag again.
#[derive(Component, Default, Clone)]
pub struct RefreshReferencesButton;

/// Fills the rows from the cache, and says when there is nothing to fill them from.
///
/// A row leaves the tab ring by its [`TabIndex`] going negative, not by being hidden:
/// bevy gathers focusables on tab index alone and looks at neither `Visibility` nor
/// `Node.display`, so eight blank rows would otherwise be eight tab stops that answer
/// Enter. Writing the value rather than adding and removing the component also keeps the
/// row in one archetype.
pub fn show_references(
    mut commands: Commands,
    references: Res<References>,
    zk: Res<ZkState>,
    mut rows: Query<(Entity, &mut ReferenceRow, &Children, Has<InteractionDisabled>)>,
    mut refreshes: Query<
        (Entity, Has<InteractionDisabled>),
        (With<RefreshReferencesButton>, Without<ReferenceRow>),
    >,
    mut headings: Query<&mut Text, With<ReferencesHeading>>,
    mut captions: Query<&mut Text, Without<ReferencesHeading>>,
    mut indices: Query<&mut TabIndex>,
) {
    let view = references.view(&zk);
    let found: &[Reference] = match view {
        View::Found(found) => found,
        _ => &[],
    };
    let keep = matches!(view, View::Looking);

    for mut text in headings.iter_mut() {
        let shown = heading(view, references.tag.as_deref());
        if text.0 != shown {
            text.0 = shown;
        }
    }

    for (entity, mut row, children, disabled) in rows.iter_mut() {
        if keep && row.path.is_some() {
            set_pressable(&mut commands, &mut indices, entity, disabled, false);
            continue;
        }
        let note = found.get(row.slot);
        let path = note.map(|note| note.path.clone());
        if row.path != path {
            row.path = path;
        }
        for child in children.iter() {
            if let Ok(mut text) = captions.get_mut(child) {
                let shown = note.map(caption).unwrap_or_default();
                if text.0 != shown {
                    text.0 = shown;
                }
            }
        }
        set_pressable(&mut commands, &mut indices, entity, disabled, note.is_some());
    }

    let refreshable = matches!(view, View::Found(_) | View::Failed(_));
    for (entity, disabled) in refreshes.iter_mut() {
        if disabled == refreshable {
            if refreshable {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
    }
}

fn heading(view: View<'_>, tag: Option<&str>) -> String {
    match view {
        View::Unavailable => "zk is not installed, so notes are unavailable".to_owned(),
        View::NothingSelected => "nothing selected".to_owned(),
        View::NoNoteYet => "no note yet — Create note makes one".to_owned(),
        View::Looking => format!("looking for {}…", tag.unwrap_or("references")),
        View::Failed(why) => format!("the query failed: {why}"),
        View::Found([]) => format!("nothing references {}", tag.unwrap_or("this")),
        View::Found(found) => {
            let tag = tag.unwrap_or("this");
            match found.len().checked_sub(REFERENCE_ROWS) {
                Some(hidden) if hidden > 0 => format!(
                    "{} notes reference {tag} — {hidden} not shown",
                    found.len()
                ),
                _ => format!("{} referencing {tag}", found.len()),
            }
        }
    }
}

fn caption(note: &Reference) -> String {
    let title = if note.title.is_empty() {
        note.path.as_str()
    } else {
        note.title.as_str()
    };
    let lead: String = note.lead.chars().take(LEAD_CHARS).collect();
    if lead.is_empty() {
        title.to_owned()
    } else {
        format!("{title} — {lead}")
    }
}

const LEAD_CHARS: usize = 80;

fn set_pressable(
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

fn reference_row(slot: usize) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("") ThemedText },
        }
        ReferenceRow { slot: {slot}, path: {None} }
        InteractionDisabled
        TabIndex(-1)
        ThemeBackgroundColor(tokens::WINDOW_BG)
        on(|activate: On<Activate>,
            rows: Query<&ReferenceRow>,
            campaign: Res<OpenCampaign>,
            mut status: ResMut<StatusMessage>| {
            let Ok(row) = rows.get(activate.event_target()) else {
                return;
            };
            let Some(path) = row.path.as_deref() else {
                return;
            };
            crate::notes::open(&campaign, &mut status, path);
        })
    }
}

fn refresh_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("Refresh") ThemedText },
        }
        RefreshReferencesButton
        InteractionDisabled
        on(|_activate: On<Activate>, mut references: ResMut<References>| {
            references.refresh();
        })
    }
}
